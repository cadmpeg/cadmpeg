// SPDX-License-Identifier: Apache-2.0

use super::*;

fn parse_svg(body: &str) -> (TemplateGeometry, Vec<LossNote>) {
    let source = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:freecad="http://www.freecadweb.org/wiki/index.php?title=Svg_Namespace" width="210mm" height="297mm" viewBox="0 0 210 297">{body}</svg>"#
    );
    let xml = Document::parse(&source).unwrap();
    let size = DrawingSheet {
        width_mm: PositiveReal::new(210.0).unwrap(),
        height_mm: PositiveReal::new(297.0).unwrap(),
        fields: Vec::new(),
    };
    let editable = BTreeMap::from([("Title", "Resolved title")]);
    let mut losses = Vec::new();
    let output = parse(&xml, &size, &editable, "Template", &mut losses).unwrap();
    (output, losses)
}

#[test]
fn normalizes_lines_rectangles_paths_and_resolved_text() {
    let (geometry, _) = parse_svg(
        r#"<rect x="10" y="10" width="190" height="277"/><path d="M110 267 H200 M110 267 V287"/><text x="115" y="278" font-size="4" freecad:editable="Title"><tspan id="field">Old title</tspan></text>"#,
    );
    assert_eq!(geometry.curves.len(), 6);
    let corners = [
        [10.0, 287.0],
        [200.0, 287.0],
        [200.0, 10.0],
        [10.0, 10.0],
        [10.0, 287.0],
    ];
    for (curve, pair) in geometry.curves[..4].iter().zip(corners.windows(2)) {
        assert!(
            matches!(curve, DrawingCurveGeometry::Line {start,end} if *start == pair[0] && *end == pair[1])
        );
    }
    assert!(
        matches!(&geometry.curves[4], DrawingCurveGeometry::Line {start,end} if *start == [110.0,30.0] && *end == [200.0,30.0])
    );
    assert_eq!(geometry.texts[0].text, "Resolved title");
    assert_eq!(geometry.texts[0].position, [115.0, 19.0]);
    assert_eq!(geometry.texts[0].height.get(), 4.0);
}

#[test]
fn unsupported_subtrees_never_emit_untransformed_or_partial_geometry() {
    let (geometry, losses) = parse_svg(
        r#"<g transform="translate(10,10)"><line x2="10"/></g><path d="M0 0 L10 10 Q20 20 30 30"/><rect width="10" height="10" rx="2"/><image href="image.png"/><text x="1" y="2" font-size="3"><tspan dx="1">Moved</tspan></text>"#,
    );
    assert!(geometry.curves.is_empty());
    assert!(geometry.texts.is_empty());
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.message.contains("unsupported or invalid SVG"))
            .count(),
        5
    );
}

#[test]
fn paths_and_polyline_admission_refuse_incomplete_coordinates() {
    for path in [
        "M0 0 L1",
        "M0 0 H",
        "M0 0 Z L2",
        "L1 2",
        "M0 0 C1 2 3 4 5 6",
        "M0 0 L1 1 L",
        "M0 0 L1 1 A",
    ] {
        assert!(super::path(path, 297.0).is_none(), "{path}");
    }
    let (geometry, losses) =
        parse_svg(r#"<polyline points="0,0 10,20 30,40"/><polygon points="0 0 1"/>"#);
    assert_eq!(geometry.curves.len(), 2);
    assert!(losses.iter().any(|loss| loss.message.contains("polygon")));
}

#[test]
fn normalizes_circular_arcs_without_approximation() {
    let (geometry, _) =
        parse_svg(r#"<circle cx="50" cy="50" r="10"/><path d="M20 20 A10 10 0 0 1 30 30"/>"#);
    assert!(
        matches!(&geometry.curves[0],DrawingCurveGeometry::Circle { center, radius } if *center == [50.0,247.0] && radius.get() == 10.0)
    );
    assert!(
        matches!(&geometry.curves[1],DrawingCurveGeometry::Arc { center, radius, start, middle, end } if *center == [20.0,267.0] && radius.get() == 10.0 && *start == [20.0,277.0] && *end == [30.0,267.0] && middle[0] > 20.0 && middle[1] > 267.0)
    );
    for refused in [
        "M0 0 A5 6 0 0 1 5 5",
        "M0 0 A1 1 0 0 1 10 10",
        "M0 0 A5 5 0 2 1 5 5",
    ] {
        assert!(path(refused, 297.0).is_none());
    }
}

#[test]
fn stylesheet_effects_refuse_the_whole_template() {
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="210mm" height="297mm" viewBox="0 0 210 297"><style>line { transform: translate(10px,10px) }</style><line x2="10"/></svg>"#;
    let xml = Document::parse(source).unwrap();
    let size = DrawingSheet {
        width_mm: PositiveReal::new(210.0).unwrap(),
        height_mm: PositiveReal::new(297.0).unwrap(),
        fields: Vec::new(),
    };
    assert!(parse(&xml, &size, &BTreeMap::new(), "Template", &mut Vec::new()).is_err());
}

#[test]
fn namespace_aliases_and_animated_geometry_never_use_default_coordinates() {
    let (geometry, losses) = parse_svg(
        r#"<rect freecad:x="10" width="20" height="20"/><line x2="10"><animate attributeName="x2" to="20"/></line><text x="1" y="2" font-size="3" editable="Title">Old</text>"#,
    );
    assert!(geometry.curves.is_empty());
    assert!(geometry.texts.is_empty());
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.message.contains("unsupported or invalid SVG"))
            .count(),
        3
    );
}

#[test]
fn foreign_spans_have_no_svg_text_projection() {
    let (geometry, losses) = parse_svg(
        r#"<text xmlns:foreign="urn:foreign" x="1" y="2" font-size="3"><foreign:tspan>Foreign text</foreign:tspan></text>"#,
    );
    assert!(geometry.texts.is_empty());
    assert!(losses.iter().any(|loss| loss
        .message
        .contains("unsupported or invalid SVG text subtree")));
}
