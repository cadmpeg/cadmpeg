// SPDX-License-Identifier: Apache-2.0
use crate::FcstdCodec;
use cadmpeg_ir::drawings::{DrawingCurveGeometry, DrawingKind};
use cadmpeg_ir::semantic_annotations::{DimensionKind, SemanticAnnotationKind};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::io::Cursor;
const LENGTH_TOLERANCE: f64 = 1.0e-12;
const SCALE_TOLERANCE: f64 = 1.0e-12;

#[test]
fn authored_drawing_keeps_page_view_dimension_and_note() {
    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw_annotations.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let model = &decoded.ir().model;
    let drawing = |name: &str| {
        model
            .drawings
            .iter()
            .find(|d| d.object.ends_with(&format!("#{name}")))
            .unwrap()
    };
    for (name, kind) in [
        ("Page", DrawingKind::Page),
        ("Template", DrawingKind::Template),
        ("View", DrawingKind::View),
        ("Dimension", DrawingKind::Dimension),
        ("Note", DrawingKind::Annotation),
        ("Symbol", DrawingKind::Symbol),
    ] {
        assert_eq!(drawing(name).kind, kind);
    }
    let page = drawing("Page");
    assert_eq!(page.template.as_ref(), Some(&drawing("Template").id));
    let views = &page.relationships["Views"];
    assert_eq!(views.len(), 4);
    for (reference, name) in views.iter().zip(["View", "Dimension", "Note", "Symbol"]) {
        assert_eq!(reference.local_target(), Some(drawing(name).id.as_str()));
    }
    let template = drawing("Template");
    let sheet = template.sheet.as_ref().unwrap();
    assert!((sheet.width_mm.get() - 210.0).abs() < LENGTH_TOLERANCE);
    assert!((sheet.height_mm.get() - 297.0).abs() < LENGTH_TOLERANCE);
    assert!(template
        .assets
        .iter()
        .any(|name| name.ends_with("cc0_template.svg")));
    let entries = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::EntryRecord>("entries")
        .unwrap();
    let svg = entries
        .iter()
        .find(|entry| entry.name() == "cc0_template.svg")
        .unwrap();
    assert_eq!(svg.data(), br#"<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297"><rect x="5" y="5" width="200" height="287" fill="none" stroke="black"/><text x="10" y="288">cadmpeg CC0</text></svg>"#);
    let geometry = template.template_geometry.as_ref().unwrap();
    assert_eq!(geometry.curves.len(), 4);
    let corners = [
        [5.0, 292.0],
        [205.0, 292.0],
        [205.0, 5.0],
        [5.0, 5.0],
        [5.0, 292.0],
    ];
    for (curve, pair) in geometry.curves.iter().zip(corners.windows(2)) {
        let DrawingCurveGeometry::Line { start, end } = curve else {
            panic!("template border")
        };
        for (actual, expected) in start
            .get()
            .into_iter()
            .chain(end.get())
            .zip(pair[0].into_iter().chain(pair[1]))
        {
            assert!((actual - expected).abs() < LENGTH_TOLERANCE);
        }
    }
    let view = drawing("View");
    let [x, y] = view.position.unwrap().get();
    assert!((x - 105.0).abs() < LENGTH_TOLERANCE);
    assert!((y - 148.5).abs() < LENGTH_TOLERANCE);
    assert!((view.scale.unwrap().get() - 1.0).abs() < SCALE_TOLERANCE);
    assert_eq!(
        view.relationships["Source"][0].local_target(),
        Some("fcstd:native:object#Model")
    );
    let dim = model
        .semantic_annotations
        .iter()
        .find(|a| a.object.ends_with("#Dimension"))
        .unwrap();
    assert_eq!(dim.kind, SemanticAnnotationKind::Dimension);
    assert!(matches!(
        dim.dimension_kind,
        Some(DimensionKind::Distance {})
    ));
    assert_eq!(dim.format.as_deref(), Some("%.2w"));
    assert_eq!(dim.references["References2D"][0].subelements, ["Edge1"]);
    assert_eq!(
        dim.references["References2D"][0].local_target(),
        Some(drawing("View").id.as_str())
    );
    let [x, y, z] = dim.position.unwrap().get();
    for (actual, expected) in [x, y, z].into_iter().zip([80.0, 125.0, 0.0]) {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    let note = model
        .semantic_annotations
        .iter()
        .find(|a| a.object.ends_with("#Note"))
        .unwrap();
    assert_eq!(note.kind, SemanticAnnotationKind::Text);
    assert_eq!(note.text, ["CC0 INSPECTION NOTE"]);
    for (actual, expected) in note
        .position
        .unwrap()
        .get()
        .into_iter()
        .zip([105.0, 148.5, 0.0])
    {
        assert!((actual - expected).abs() < LENGTH_TOLERANCE);
    }
    let properties = decoded
        .ir()
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::PropertyRecord>("properties")
        .unwrap();
    assert!(properties.iter().any(|property| {
        property.owner == crate::native::native_id("object", "Symbol")
            && property.name == "Symbol"
            && property.values().iter().any(|value| {
                value
                    .attributes
                    .get("value")
                    .is_some_and(|path| path.ends_with("cc0_template.svg"))
            })
    }));
}

#[test]
fn authored_drawing_box_matches_source_dimensions() {
    use cadmpeg_measure::test_support::round_trip::measured;
    const VOLUME_TOLERANCE: f64 = 1.0e-6;
    const AREA_TOLERANCE: f64 = 1.0e-6;
    const BOX_TOLERANCE: f64 = 1.0e-6;
    let bytes =
        include_bytes!("../../../../corpus/freecad_fcstd/fixtures/techdraw_annotations.FCStd");
    let decoded = FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let report = measured("techdraw_annotations", decoded.ir()).unwrap();
    let [body] = report.bodies().as_slice() else {
        panic!("one model box")
    };
    let volume = body
        .volume()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    let area = body
        .area()
        .as_ref()
        .unwrap()
        .measured()
        .unwrap()
        .value()
        .value
        .get();
    assert!((volume - 25.0 * 15.0 * 8.0).abs() < VOLUME_TOLERANCE);
    assert!((area - 2.0 * (25.0 * 15.0 + 25.0 * 8.0 + 15.0 * 8.0)).abs() < AREA_TOLERANCE);
    let bbox = body.bbox().measured().unwrap().value();
    for (actual, expected) in bbox
        .min
        .coordinates()
        .into_iter()
        .chain(bbox.max.coordinates())
        .zip([0.0, 0.0, 0.0, 25.0, 15.0, 8.0])
    {
        assert!((actual.get() - expected).abs() < BOX_TOLERANCE);
    }
}
