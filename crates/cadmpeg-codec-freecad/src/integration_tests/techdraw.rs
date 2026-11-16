// SPDX-License-Identifier: Apache-2.0

use super::decode;
use crate::test_support::test_archive::assert_valid_document;
use cadmpeg_ir::drawings::DrawingKind;
use cadmpeg_ir::semantic_annotations::{DimensionKind, SemanticAnnotationKind};

const BOX_VOLUME_TOLERANCE: f64 = 1.0e-5;
const BOX_AREA_TOLERANCE: f64 = 1.0e-5;
const BOX_BOUNDS_TOLERANCE: f64 = 1.0e-6;
const SHAPE_VOLUME_TOLERANCE: f64 = 1.0e-5;
const SHAPE_AREA_TOLERANCE: f64 = 1.0e-5;
const SHAPE_BOUNDS_TOLERANCE: f64 = 1.0e-6;
const WIRE_LENGTH_TOLERANCE: f64 = 1.0e-6;

fn assert_authored_box_measures(ir: &cadmpeg_ir::document::CadIr, label: &str) {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Box"))
        .unwrap();
    let [body_id] = feature.evaluation.outputs().as_slice() else {
        panic!("Box feature must produce one body")
    };
    let report = cadmpeg_measure::test_support::round_trip::measured(label, ir).unwrap();
    let body = report
        .bodies()
        .iter()
        .find(|body| body.body() == body_id.as_str())
        .unwrap();
    let volume = body
        .volume()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!(
        (volume - 6000.0).abs() < BOX_VOLUME_TOLERANCE,
        "{label}: {volume}"
    );
    let area = body
        .area()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!(
        (area - 2200.0).abs() < BOX_AREA_TOLERANCE,
        "{label}: {area}"
    );
    let bounds = body.bbox().measured().unwrap().value();
    for (actual, expected) in bounds
        .min
        .coordinates()
        .into_iter()
        .chain(bounds.max.coordinates())
        .zip([0.0, 0.0, 0.0, 30.0, 20.0, 10.0])
    {
        assert!(
            (actual.get() - expected).abs() < BOX_BOUNDS_TOLERANCE,
            "{label}: bound {} expected {expected}",
            actual.get()
        );
    }
}

fn assert_authored_triangle_measure(
    ir: &cadmpeg_ir::document::CadIr,
    report: &cadmpeg_measure::report::MeasureReport,
    label: &str,
) {
    let feature = ir
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Triangle"))
        .unwrap_or_else(|| panic!("{label}: missing Triangle feature"));
    let [body_id] = feature.evaluation.outputs().as_slice() else {
        panic!("{label}: Triangle feature must produce one body")
    };
    let triangle = report
        .bodies()
        .iter()
        .find(|body| body.body() == body_id.as_str())
        .unwrap_or_else(|| panic!("{label}: no measured body for Triangle output"));
    let length = triangle
        .length()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    let expected_length = 50.0 + 1700.0_f64.sqrt();
    assert!(
        (length - expected_length).abs() < WIRE_LENGTH_TOLERANCE,
        "{label} Triangle: length {length}, expected {expected_length}"
    );
    let bounds = triangle.bbox().measured().unwrap().value();
    for (actual, expected) in bounds
        .min
        .coordinates()
        .into_iter()
        .chain(bounds.max.coordinates())
        .zip([10.0, 20.0, 0.0, 50.0, 30.0, 0.0])
    {
        assert!(
            (actual.get() - expected).abs() < SHAPE_BOUNDS_TOLERANCE,
            "{label} Triangle: bound {}, expected {expected}",
            actual.get()
        );
    }
}

fn assert_authored_fixture_shape_measures(
    ir: &cadmpeg_ir::document::CadIr,
    label: &str,
    box_length: f64,
    cylinder_radius: f64,
) {
    let report = cadmpeg_measure::test_support::round_trip::measured(label, ir).unwrap();
    let body_for_feature = |name: &str| {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{label}: missing {name} feature"));
        let [body_id] = feature.evaluation.outputs().as_slice() else {
            panic!("{label}: {name} feature must produce one body")
        };
        report
            .bodies()
            .iter()
            .find(|body| body.body() == body_id.as_str())
            .unwrap_or_else(|| panic!("{label}: no measured body for {name} output"))
    };

    let box_body = body_for_feature("Box");
    let cylinder_body = body_for_feature("Cylinder");
    for (name, body, expected_volume, expected_area, expected_bounds) in [
        (
            "Box",
            box_body,
            box_length * 20.0 * 10.0,
            2.0 * (box_length * 20.0 + box_length * 10.0 + 20.0 * 10.0),
            [0.0, 0.0, 0.0, box_length, 20.0, 10.0],
        ),
        (
            "Cylinder",
            cylinder_body,
            std::f64::consts::PI * cylinder_radius.powi(2) * 10.0,
            2.0 * std::f64::consts::PI * cylinder_radius * (cylinder_radius + 10.0),
            [
                -cylinder_radius,
                -cylinder_radius,
                0.0,
                cylinder_radius,
                cylinder_radius,
                10.0,
            ],
        ),
    ] {
        let volume = body
            .volume()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (volume - expected_volume).abs() < SHAPE_VOLUME_TOLERANCE,
            "{label} {name}: volume {volume}, expected {expected_volume}"
        );
        let area = body
            .area()
            .as_ref()
            .unwrap()
            .measured()
            .unwrap()
            .value()
            .value
            .get();
        assert!(
            (area - expected_area).abs() < SHAPE_AREA_TOLERANCE,
            "{label} {name}: area {area}, expected {expected_area}"
        );
        let bounds = body.bbox().measured().unwrap().value();
        for (actual, expected) in bounds
            .min
            .coordinates()
            .into_iter()
            .chain(bounds.max.coordinates())
            .zip(expected_bounds)
        {
            assert!(
                (actual.get() - expected).abs() < SHAPE_BOUNDS_TOLERANCE,
                "{label} {name}: bound {}, expected {expected}",
                actual.get()
            );
        }
    }

    assert_authored_triangle_measure(ir, &report, label);
}

fn assert_authored_view(
    ir: &cadmpeg_ir::document::CadIr,
    name: &str,
    source: &str,
    position: [f64; 2],
    scale: f64,
    direction: [f64; 3],
    rotation_degrees: f64,
) {
    let view = ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with(&format!("#{name}")))
        .unwrap();
    assert_eq!(view.kind, DrawingKind::View, "{name}");
    assert_eq!(view.position.unwrap().get(), position, "{name}");
    assert_eq!(view.scale.unwrap().get(), scale, "{name}");
    assert_eq!(view.direction.unwrap().as_raw(), &direction, "{name}");
    assert_eq!(
        view.x_direction.unwrap().as_raw(),
        &[1.0, 0.0, 0.0],
        "{name}"
    );
    assert_eq!(
        view.rotation_degrees.unwrap().get(),
        rotation_degrees,
        "{name}"
    );
    assert_eq!(
        view.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", source).as_str()),
        "{name} source"
    );
}

fn assert_authored_dimension(
    ir: &cadmpeg_ir::document::CadIr,
    name: &str,
    position: [f64; 3],
    expected_references: &[(&str, &[&str])],
    expected_anchors: &[[f64; 2]],
) {
    let drawing = |name: &str| {
        ir.model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with(&format!("#{name}")))
            .unwrap()
    };
    let page = ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Page)
        .unwrap();
    let dimension_drawing = drawing(name);
    assert_eq!(dimension_drawing.kind, DrawingKind::Dimension, "{name}");
    assert!(
        page.relationships["Views"]
            .iter()
            .any(|reference| reference.local_target() == Some(dimension_drawing.id.as_str())),
        "Page.Views owns {name}"
    );

    let annotation = ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with(&format!("#{name}")))
        .unwrap();
    assert_eq!(annotation.position.unwrap().get(), position, "{name}");
    let references = &annotation.references["References2D"];
    assert_eq!(references.len(), expected_references.len(), "{name}");
    for (reference, (view_name, subelements)) in references.iter().zip(expected_references) {
        assert_eq!(
            reference.local_target(),
            Some(drawing(view_name).id.as_str()),
            "{name} reference view"
        );
        assert_eq!(
            reference
                .subelements
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            *subelements,
            "{name} reference subelements"
        );
    }
    let anchors = annotation
        .reference_anchors
        .iter()
        .map(|point| **point)
        .collect::<Vec<_>>();
    assert_eq!(anchors.as_slice(), expected_anchors, "{name} anchors");
}

#[test]
fn physical_page_template_and_saved_dimension_are_exact() {
    let result =
        decode(include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd").to_vec());
    let ir = result.ir();
    assert_valid_document(ir);
    let page = ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Page)
        .unwrap();
    let drawing = |name: &str| {
        ir.model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with(&format!("#{name}")))
            .unwrap()
    };
    let size = ir
        .model
        .drawings
        .iter()
        .find(|drawing| Some(&drawing.id) == page.template.as_ref())
        .unwrap()
        .sheet
        .as_ref()
        .unwrap();
    assert_eq!((size.width_mm.get(), size.height_mm.get()), (210.0, 297.0));
    let template = ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Template)
        .unwrap();
    let geometry = template.template_geometry.as_ref().unwrap();
    assert_eq!(geometry.curves.len(), 6);
    assert_eq!(geometry.texts[0].text, "Synthetic box");
    assert_eq!(geometry.texts[0].position, [115.0, 19.0]);
    assert_eq!(geometry.texts[0].height.unwrap().get(), 4.0);
    let dimension = ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.dimension_kind.is_some())
        .unwrap();
    assert_eq!(dimension.value.unwrap().get(), 30.0);
    assert_eq!(dimension.position.unwrap(), [65.0, 160.0, 0.0]);
    assert_eq!(
        dimension.dimension_kind.clone().unwrap(),
        DimensionKind::HorizontalDistance {}
    );
    let [reference] = dimension.references["References2D"].as_slice() else {
        panic!("Length must have one 2D reference")
    };
    assert_eq!(reference.local_target(), Some(drawing("Front").id.as_str()));
    assert_eq!(reference.subelements, ["Edge3"]);
    assert_eq!(
        dimension
            .reference_anchors
            .iter()
            .map(|p| **p)
            .collect::<Vec<_>>(),
        [[35.0, 190.0], [95.0, 190.0]]
    );
    let page_views = &page.relationships["Views"];
    assert_eq!(page_views.len(), 4);
    for (reference, name) in page_views.iter().zip(["Front", "Top", "Length", "Note"]) {
        assert_eq!(reference.local_target(), Some(drawing(name).id.as_str()));
    }
    for (name, position, scale, direction) in [
        ("Front", [65.0, 180.0], 2.0, [0.0, -1.0, 0.0]),
        ("Top", [65.0, 230.0], 1.0, [0.0, 0.0, 1.0]),
    ] {
        let view = drawing(name);
        assert_eq!(view.kind, DrawingKind::View);
        assert_eq!(view.position.unwrap().get(), position);
        assert_eq!(view.scale.unwrap().get(), scale);
        assert_eq!(view.direction.unwrap().as_raw(), &direction);
        assert_eq!(
            view.relationships["Source"][0].local_target(),
            Some(crate::native::native_id("object", "Box").as_str())
        );
    }
    let note = ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .unwrap();
    assert_eq!(note.kind, SemanticAnnotationKind::Text);
    assert_eq!(note.text, ["Synthetic box", "30 x 20 x 10 mm"]);
    assert_eq!(note.position.unwrap().get(), [120.0, 100.0, 0.0]);
    assert_authored_box_measures(ir, "techdraw");
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss
                .message
                .contains("projected view edges are runtime geometry"))
            .count(),
        2
    );
}

#[test]
fn authored_techdraw_views_keep_page_ownership_and_saved_parameters() {
    let result = decode(
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-views.FCStd").to_vec(),
    );
    let ir = result.ir();
    assert_valid_document(ir);
    let drawing = |name: &str| {
        ir.model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with(&format!("#{name}")))
            .unwrap()
    };
    let page = drawing("Page");
    let page_views = &page.relationships["Views"];
    for name in [
        "Front",
        "Top",
        "Length",
        "Note",
        "Projection",
        "Section",
        "Detail",
        "Euclidean",
        "Vertical",
        "TriangleView",
    ] {
        assert!(
            page_views
                .iter()
                .any(|reference| reference.local_target() == Some(drawing(name).id.as_str())),
            "Page.Views owns {name}"
        );
    }
    let note = ir
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .unwrap();
    assert_eq!(note.kind, SemanticAnnotationKind::Text);
    assert_eq!(note.text, ["Synthetic box", "30 x 20 x 10 mm"]);
    assert_eq!(note.position.unwrap().get(), [120.0, 100.0, 0.0]);

    let triangle_view = drawing("TriangleView");
    assert_eq!(triangle_view.kind, DrawingKind::View);
    assert_eq!(triangle_view.position.unwrap().get(), [105.0, 148.5]);
    assert_eq!(triangle_view.scale.unwrap().get(), 2.0);
    assert_eq!(triangle_view.direction.unwrap().as_raw(), &[0.0, 0.0, 1.0]);
    assert_eq!(
        triangle_view.x_direction.unwrap().as_raw(),
        &[1.0, 0.0, 0.0]
    );
    assert_eq!(triangle_view.rotation_degrees.unwrap().get(), 30.0);
    assert_eq!(
        triangle_view.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", "Triangle").as_str())
    );
    for (name, kind, value) in [
        ("Euclidean", DimensionKind::Distance {}, 30.0),
        ("Vertical", DimensionKind::VerticalDistance {}, 10.0),
    ] {
        let annotation = ir
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with(&format!("#{name}")))
            .unwrap();
        assert_eq!(annotation.dimension_kind.as_ref(), Some(&kind), "{name}");
        assert_eq!(annotation.value.unwrap().get(), value, "{name}");
    }

    let projection = drawing("Projection");
    assert_eq!(
        projection.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", "Box").as_str())
    );

    let front = drawing("Front");
    assert_eq!(front.position.unwrap().get(), [65.0, 180.0]);
    assert_eq!(front.scale.unwrap().get(), 2.0);
    assert_eq!(front.direction.unwrap().as_raw(), &[0.0, -1.0, 0.0]);
    assert_eq!(
        front.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", "Box").as_str())
    );
    let top = drawing("Top");
    assert_eq!(top.position.unwrap().get(), [65.0, 230.0]);
    assert_eq!(top.scale.unwrap().get(), 1.0);
    assert_eq!(top.direction.unwrap().as_raw(), &[0.0, 0.0, 1.0]);
    assert_eq!(
        top.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", "Box").as_str())
    );

    let section = drawing("Section");
    assert_eq!(
        section.relationships["Source"][0].local_target(),
        Some(crate::native::native_id("object", "Box").as_str())
    );
    assert_eq!(
        section.relationships["BaseView"][0].local_target(),
        Some(front.id.as_str())
    );
    let detail = drawing("Detail");
    assert_eq!(
        detail.relationships["BaseView"][0].local_target(),
        Some(front.id.as_str())
    );
    let properties = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    let section_normal = properties
        .iter()
        .find(|property| {
            property.owner == crate::native::native_id("object", "Section")
                && property.name == "SectionNormal"
        })
        .unwrap();
    assert_eq!(section_normal.type_name, "App::PropertyVector");
    assert_eq!(section_normal.values().len(), 1);
    assert_eq!(
        ["valueX", "valueY", "valueZ"].map(|axis| {
            section_normal.values()[0].attributes[axis]
                .parse::<f64>()
                .unwrap()
        }),
        [1.0, 0.0, 0.0]
    );
    assert_authored_box_measures(ir, "techdraw-views");
    let report = cadmpeg_measure::test_support::round_trip::measured("techdraw-views", ir).unwrap();
    assert_authored_triangle_measure(ir, &report, "techdraw-views");
}

#[test]
fn drawing_only_pages_keep_independent_physical_sheets() {
    let result = decode(
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-pages.FCStd").to_vec(),
    );
    assert_valid_document(result.ir());
    let pages = result
        .ir()
        .model
        .drawings
        .iter()
        .filter(|drawing| drawing.kind == DrawingKind::Page)
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    let drawing = |name: &str| {
        result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with(&format!("#{name}")))
            .unwrap()
    };
    let first_template = drawing("FirstTemplate");
    let second_template = drawing("SecondTemplate");
    assert_ne!(first_template.id, second_template.id);
    for (name, x, template) in [
        ("First", 20.0, first_template),
        ("Second", 40.0, second_template),
    ] {
        let page = pages
            .iter()
            .find(|page| page.object.ends_with(&format!("#{name}")))
            .unwrap();
        assert_eq!(
            page.template.as_ref(),
            Some(&template.id),
            "{name} template"
        );
        assert_eq!(template.kind, DrawingKind::Template, "{name} template");
        let size = template.sheet.as_ref().unwrap();
        assert_eq!((size.width_mm.get(), size.height_mm.get()), (210.0, 297.0));
        let geometry = template.template_geometry.as_ref().unwrap();
        assert_eq!(geometry.texts.len(), 1, "{name} template title");
        assert_eq!(
            geometry.texts[0].text, "Synthetic box",
            "{name} template title"
        );
        assert_eq!(
            geometry.texts[0].position,
            [115.0, 19.0],
            "{name} template title"
        );
        assert_eq!(
            geometry.texts[0].height.unwrap().get(),
            4.0,
            "{name} template title"
        );
        let note_name = format!("{name}Note");
        let note = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with(&format!("#{note_name}")))
            .unwrap();
        assert_eq!(note.kind, SemanticAnnotationKind::Text);
        assert_eq!(note.text, [name]);
        assert_eq!(note.position.unwrap().get(), [x, 30.0, 0.0]);
        assert_eq!(note.text_style.height.unwrap().get(), 5.0);
        let view = result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| drawing.object.ends_with(&format!("#{note_name}")))
            .unwrap();
        assert_eq!(view.kind, DrawingKind::Annotation);
        let [reference] = page.relationships["Views"].as_slice() else {
            panic!("{name} page must own exactly one view")
        };
        assert_eq!(reference.local_target(), Some(view.id.as_str()));
    }
}

#[test]
fn expanded_templates_dimensions_and_view_families_keep_exact_data_and_losses() {
    use cadmpeg_ir::drawings::DrawingCurveGeometry;
    let result = decode(
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-dimensions.FCStd")
            .to_vec(),
    );
    assert_valid_document(result.ir());
    assert_authored_fixture_shape_measures(result.ir(), "techdraw-dimensions", 30.0, 7.0);
    let model = &result.ir().model;
    for (name, kind, value, anchor_count) in [
        ("Length", DimensionKind::HorizontalDistance {}, 30.0, 2),
        ("Euclidean", DimensionKind::Distance {}, 30.0, 2),
        ("Vertical", DimensionKind::VerticalDistance {}, 10.0, 2),
        ("Radius", DimensionKind::Radius {}, 7.0, 2),
        ("Diameter", DimensionKind::Diameter {}, 14.0, 2),
        ("Angle", DimensionKind::Angle {}, 90.0, 0),
    ] {
        let annotation = model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with(&format!("#{name}")))
            .unwrap();
        assert_eq!(annotation.value.unwrap().get(), value, "{name}");
        assert_eq!(annotation.dimension_kind.as_ref(), Some(&kind));
        assert_eq!(annotation.reference_anchors.len(), anchor_count);
    }
    for (name, source, position, scale, direction, rotation) in [
        ("Front", "Box", [65.0, 180.0], 2.0, [0.0, -1.0, 0.0], 0.0),
        (
            "TriangleView",
            "Triangle",
            [105.0, 148.5],
            2.0,
            [0.0, 0.0, 1.0],
            30.0,
        ),
        (
            "Circular",
            "Cylinder",
            [105.0, 148.5],
            1.0,
            [0.0, 0.0, 1.0],
            0.0,
        ),
    ] {
        assert_authored_view(
            result.ir(),
            name,
            source,
            position,
            scale,
            direction,
            rotation,
        );
    }
    for (name, position, references, anchors) in [
        (
            "Length",
            [65.0, 160.0, 0.0],
            &[("Front", &["Edge3"][..])][..],
            &[[35.0, 190.0], [95.0, 190.0]][..],
        ),
        (
            "Euclidean",
            [90.0, 160.0, 0.0],
            &[("Front", &["Edge3"][..])][..],
            &[[35.0, 190.0], [95.0, 190.0]][..],
        ),
        (
            "Vertical",
            [90.0, 160.0, 0.0],
            &[("Front", &["Edge1"][..])][..],
            &[[95.0, 170.0], [95.0, 190.0]][..],
        ),
        (
            "Radius",
            [140.0, 200.0, 0.0],
            &[("Circular", &["Edge0"][..])][..],
            &[[105.0, 148.5], [112.0, 148.5]][..],
        ),
        (
            "Diameter",
            [140.0, 200.0, 0.0],
            &[("Circular", &["Edge0"][..])][..],
            &[[98.0, 148.5], [112.0, 148.5]][..],
        ),
        (
            "Angle",
            [140.0, 200.0, 0.0],
            &[("Front", &["Edge1"][..]), ("Front", &["Edge3"][..])][..],
            &[][..],
        ),
    ] {
        assert_authored_dimension(result.ir(), name, position, references, anchors);
    }
    let template = model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Template)
        .unwrap()
        .template_geometry
        .as_ref()
        .unwrap();
    assert_eq!(template.texts[0].text, "Edited title");
    assert_eq!(template.curves.len(), 8);
    assert!(
        matches!(&template.curves[6],DrawingCurveGeometry::Circle { center, radius } if *center == [50.0,247.0] && radius.get() == 10.0)
    );
    assert!(
        matches!(&template.curves[7],DrawingCurveGeometry::Arc { center, start, end, .. } if *center == [20.0,267.0] && *start == [20.0,277.0] && *end == [30.0,267.0])
    );
    for runtime in [
        "TechDraw::DrawViewPart",
        "TechDraw::DrawProjGroup",
        "TechDraw::DrawProjGroupItem",
        "TechDraw::DrawViewSection",
        "TechDraw::DrawViewDetail",
    ] {
        for view in model
            .drawings
            .iter()
            .filter(|drawing| drawing.runtime_type == runtime)
        {
            assert!(
                result
                    .report()
                    .losses
                    .iter()
                    .any(|loss| loss.message.contains(&view.object)
                        && loss
                            .message
                            .contains("projected view edges are runtime geometry")),
                "{runtime}"
            );
        }
        assert!(model
            .drawings
            .iter()
            .any(|drawing| drawing.runtime_type == runtime));
    }
}

#[test]
fn refused_dimension_measurements_keep_the_kind_placement_and_explicit_loss() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    for (from, to) in [
        (
            "<Float value=\"0.0000000000000000\"/>",
            "<Float value=\"30.0000000000000000\"/>",
        ),
        ("<Bool value=\"false\"/>", "<Bool value=\"true\"/>"),
    ] {
        let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
            let source = std::str::from_utf8(bytes).unwrap();
            assert!(source.contains(from));
            source.replace(from, to).into_bytes()
        });
        let result = decode(bytes);
        assert_valid_document(result.ir());
        let dimension = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.dimension_kind.is_some())
            .unwrap();
        assert!(dimension.value.is_none());
        assert!(dimension.reference_anchors.is_empty());
        assert!(dimension.position.is_some());
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("value and anchors remain absent")));
    }
}

#[test]
fn unsupported_template_and_absent_reference_cache_report_losses() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let bytes = rewrite_entry(fixture, "techdraw-template.svg", |_| b"<svg/>".to_vec());
    let bytes = rewrite_entry(&bytes, "Length.SavedGeometry.0.brp", |_| Vec::new());
    let result = decode(bytes);
    assert_valid_document(result.ir());
    assert!(result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Template)
        .unwrap()
        .template_geometry
        .is_none());
    assert!(result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.dimension_kind.is_some())
        .unwrap()
        .value
        .is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("SVG remains in its asset")));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("value and anchors remain absent")));
}

#[test]
fn dimension_measurements_follow_changed_saved_reference_geometry() {
    let result = decode(
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-variants.FCStd")
            .to_vec(),
    );
    assert_valid_document(result.ir());
    assert_authored_fixture_shape_measures(result.ir(), "techdraw-variants", 42.0, 11.0);
    for (name, kind, expected) in [
        ("Length", DimensionKind::HorizontalDistance {}, 42.0),
        ("Euclidean", DimensionKind::Distance {}, 42.0),
        ("Vertical", DimensionKind::VerticalDistance {}, 10.0),
        ("Radius", DimensionKind::Radius {}, 11.0),
        ("Diameter", DimensionKind::Diameter {}, 22.0),
        ("Angle", DimensionKind::Angle {}, 90.0),
    ] {
        let annotation = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with(&format!("#{name}")))
            .unwrap();
        assert_eq!(annotation.value.unwrap().get(), expected);
        assert_eq!(annotation.dimension_kind.as_ref(), Some(&kind), "{name}");
    }
    for (name, source, position, scale, direction, rotation) in [
        ("Front", "Box", [65.0, 180.0], 2.0, [0.0, -1.0, 0.0], 0.0),
        (
            "TriangleView",
            "Triangle",
            [105.0, 148.5],
            2.0,
            [0.0, 0.0, 1.0],
            0.0,
        ),
        (
            "Circular",
            "Cylinder",
            [105.0, 148.5],
            1.0,
            [0.0, 0.0, 1.0],
            0.0,
        ),
    ] {
        assert_authored_view(
            result.ir(),
            name,
            source,
            position,
            scale,
            direction,
            rotation,
        );
    }
    for (name, position, references, anchors) in [
        (
            "Length",
            [65.0, 160.0, 0.0],
            &[("Front", &["Edge3"][..])][..],
            &[[23.0, 190.0], [107.0, 190.0]][..],
        ),
        (
            "Euclidean",
            [90.0, 160.0, 0.0],
            &[("Front", &["Edge3"][..])][..],
            &[[23.0, 190.0], [107.0, 190.0]][..],
        ),
        (
            "Vertical",
            [90.0, 160.0, 0.0],
            &[("Front", &["Edge1"][..])][..],
            &[[107.0, 170.0], [107.0, 190.0]][..],
        ),
        (
            "Radius",
            [140.0, 200.0, 0.0],
            &[("Circular", &["Edge0"][..])][..],
            &[[105.0, 148.5], [116.0, 148.5]][..],
        ),
        (
            "Diameter",
            [140.0, 200.0, 0.0],
            &[("Circular", &["Edge0"][..])][..],
            &[[94.0, 148.5], [116.0, 148.5]][..],
        ),
        (
            "Angle",
            [140.0, 200.0, 0.0],
            &[("Front", &["Edge1"][..]), ("Front", &["Edge3"][..])][..],
            &[][..],
        ),
    ] {
        assert_authored_dimension(result.ir(), name, position, references, anchors);
    }
}

#[test]
fn display_size_carriers_do_not_become_physical_text_heights() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
        let source = std::str::from_utf8(bytes).unwrap();
        let old = "name=\"TextSize\" type=\"App::PropertyLength\"";
        assert!(source.contains(old));
        source
            .replace(old, "name=\"TextSize\" type=\"App::PropertyFloat\"")
            .into_bytes()
    });
    let result = decode(bytes);
    assert_valid_document(result.ir());
    let note = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .unwrap();
    assert!(note.text_style.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&note.object)
            && loss.message.contains("untyped font properties")));
}

#[test]
fn exact_value_survives_absent_page_anchor_placement() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
        let source = std::str::from_utf8(bytes).unwrap();
        source
            .replace("name=\"Scale\"", "name=\"NativeScale\"")
            .into_bytes()
    });
    let result = decode(bytes);
    assert_valid_document(result.ir());
    let annotation = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.dimension_kind.is_some())
        .unwrap();
    assert_eq!(annotation.value.unwrap().get(), 30.0);
    assert!(annotation.reference_anchors.is_empty());
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("exact value retained without admitted page-space reference anchors")));
}

#[test]
fn stale_saved_references_do_not_override_current_source_geometry() {
    use crate::test_support::test_archive::rewrite_entry;
    use std::io::{Cursor, Read};
    let original = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let mut archive = zip::ZipArchive::new(Cursor::new(original)).unwrap();
    let mut stale = Vec::new();
    archive
        .by_name("Length.SavedGeometry.0.brp")
        .unwrap()
        .read_to_end(&mut stale)
        .unwrap();
    let current =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-variants.FCStd");
    let bytes = rewrite_entry(current, "Length.SavedGeometry.0.brp", |_| stale.clone());
    let result = decode(bytes);
    assert_valid_document(result.ir());
    let annotation = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Length"))
        .unwrap();
    assert!(annotation.value.is_none());
    assert!(annotation.reference_anchors.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(&annotation.object)
            && loss.message.contains("value and anchors remain absent")));
}

#[test]
fn asymmetric_saved_reference_y_is_reflected_into_page_coordinates() {
    let result = decode(
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw-variants.FCStd")
            .to_vec(),
    );
    assert_valid_document(result.ir());
    let annotation = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#TriangleBase"))
        .unwrap();
    assert_eq!(annotation.value.unwrap().get(), 40.0);
    assert_eq!(annotation.dimension_kind, Some(DimensionKind::Distance {}));
    assert_eq!(annotation.position.unwrap(), [80.0, 100.0, 0.0]);
    assert_eq!(
        annotation
            .reference_anchors
            .iter()
            .map(|point| **point)
            .collect::<Vec<_>>(),
        [[65.0, 138.5], [145.0, 138.5]]
    );
    assert_authored_dimension(
        result.ir(),
        "TriangleBase",
        [80.0, 100.0, 0.0],
        &[("TriangleView", &["Edge0"][..])],
        &[[65.0, 138.5], [145.0, 138.5]],
    );
}

#[test]
fn custom_dimension_labels_keep_precedence_during_exact_evaluation() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    for (label, expected) in [("Distance", Some(30.0)), ("Radius", None)] {
        let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
            let source = std::str::from_utf8(bytes).unwrap();
            let selected = "<Property name=\"Type\" type=\"App::PropertyEnumeration\">\n                    <Integer value=\"1\"/>";
            assert!(source.contains(selected));
            source.replace(selected, &format!("{selected}<CustomEnumList count=\"2\"><Enum value=\"DistanceY\"/><Enum value=\"{label}\"/></CustomEnumList>")).into_bytes()
        });
        let result = decode(bytes);
        assert_valid_document(result.ir());
        let dimension = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with("#Length"))
            .unwrap();
        assert_eq!(
            dimension.value.map(cadmpeg_ir::scalar::FiniteReal::get),
            expected
        );
        assert_eq!(
            dimension.dimension_kind,
            Some(if label == "Distance" {
                DimensionKind::Distance {}
            } else {
                DimensionKind::Radius {}
            })
        );
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code.local_code()
                    == "drawing.dimension-geometry-not-transferred"
                    && loss.message.contains(&dimension.object))
                .count(),
            usize::from(expected.is_none())
        );
    }
}

#[test]
fn physical_text_and_upstream_sheet_fields_have_one_owner() {
    let result =
        decode(include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd").to_vec());
    assert_valid_document(result.ir());
    let model = &result.ir().model;
    let note = model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .unwrap();
    assert_eq!(note.text_style.height.unwrap().get(), 5.0);
    assert_eq!(note.text_style.font_family.as_deref(), Some("osifont"));
    let page = model
        .drawings
        .iter()
        .find(|drawing| drawing.kind == DrawingKind::Page)
        .unwrap();
    assert!(page.sheet.is_none());
    let template = model
        .drawings
        .iter()
        .find(|drawing| Some(&drawing.id) == page.template.as_ref())
        .unwrap();
    let fields = &template.sheet.as_ref().unwrap().fields;
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name.as_str(), "Title");
    assert_eq!(fields[0].value, "Synthetic box");
    assert_eq!(
        template.template_geometry.as_ref().unwrap().texts[0].text,
        fields[0].value
    );
    assert!(model
        .drawings
        .iter()
        .filter(|drawing| drawing.kind == DrawingKind::Dimension)
        .all(|drawing| drawing.position.is_none()));
    let front = model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Front"))
        .unwrap();
    assert_eq!(front.x_direction.unwrap().as_raw(), &[1.0, 0.0, 0.0]);
}

#[test]
fn custom_measurement_modes_cannot_reuse_the_builtin_numeric_gate() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
        let source = std::str::from_utf8(bytes).unwrap();
        let selected = "<Property name=\"MeasureType\" type=\"App::PropertyEnumeration\">\n                    <Integer value=\"1\"/>";
        assert!(source.contains(selected));
        source.replace(selected, &format!("{selected}<CustomEnumList count=\"2\"><Enum value=\"Projected\"/><Enum value=\"True\"/></CustomEnumList>")).into_bytes()
    });
    let result = decode(bytes);
    assert_valid_document(result.ir());
    let dimension = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Length"))
        .unwrap();
    assert!(dimension.value.is_none());
    assert!(dimension.reference_anchors.is_empty());
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(
                |loss| loss.code.local_code() == "drawing.dimension-geometry-not-transferred"
                    && loss.message.contains(&dimension.object)
            )
            .count(),
        1
    );
}

#[test]
fn a_zero_horizontal_axis_stays_native_with_an_explicit_loss() {
    use crate::test_support::test_archive::rewrite_entry;
    let fixture = include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw.FCStd");
    let bytes = rewrite_entry(fixture, "Document.xml", |bytes| {
        let source = std::str::from_utf8(bytes).unwrap();
        let axis = "<Property name=\"XDirection\" type=\"App::PropertyVector\">\n                    <PropertyVector valueX=\"1.0000000000000000\"";
        assert!(source.contains(axis));
        source
            .replace(axis, &axis.replace("valueX=\"1.", "valueX=\"0."))
            .into_bytes()
    });
    let result = decode(bytes);
    assert_valid_document(result.ir());
    let front = result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Front"))
        .unwrap();
    assert!(front.x_direction.is_none());
    assert!(result
        .report()
        .losses
        .iter()
        .any(
            |loss| loss.code.local_code() == "drawing.view-axis-not-transferred"
                && loss.message.contains(&front.object)
        ));
    let dimension = result
        .ir()
        .model
        .semantic_annotations
        .iter()
        .find(|annotation| annotation.object.ends_with("#Length"))
        .unwrap();
    assert!(dimension.value.is_none());
}
