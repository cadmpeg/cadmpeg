// SPDX-License-Identifier: Apache-2.0

use super::decode;
use crate::test_support::test_archive::assert_valid_document;
use cadmpeg_ir::drawings::DrawingKind;
use cadmpeg_ir::semantic_annotations::DimensionKind;

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
    assert_eq!(
        dimension
            .reference_anchors
            .iter()
            .map(|p| **p)
            .collect::<Vec<_>>(),
        [[35.0, 190.0], [95.0, 190.0]]
    );
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
    for page in pages {
        let size = result
            .ir()
            .model
            .drawings
            .iter()
            .find(|drawing| Some(&drawing.id) == page.template.as_ref())
            .unwrap()
            .sheet
            .as_ref()
            .unwrap();
        assert_eq!((size.width_mm.get(), size.height_mm.get()), (210.0, 297.0));
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
    for (name, expected) in [
        ("Length", 42.0),
        ("Euclidean", 42.0),
        ("Vertical", 10.0),
        ("Radius", 11.0),
        ("Diameter", 22.0),
        ("Angle", 90.0),
    ] {
        let annotation = result
            .ir()
            .model
            .semantic_annotations
            .iter()
            .find(|annotation| annotation.object.ends_with(&format!("#{name}")))
            .unwrap();
        assert_eq!(annotation.value.unwrap().get(), expected);
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
    assert_eq!(annotation.position.unwrap(), [80.0, 100.0, 0.0]);
    assert_eq!(
        annotation
            .reference_anchors
            .iter()
            .map(|point| **point)
            .collect::<Vec<_>>(),
        [[65.0, 138.5], [145.0, 138.5]]
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
