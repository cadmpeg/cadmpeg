//! Drawing sheets, views, and visibility project to neutral drawings.

#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn decode(bytes: Vec<u8>) -> cadmpeg_test_support::EditableDecodeResult {
    cadmpeg_test_support::EditableDecodeResult::from(
        crate::IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("drawing fixture decodes"),
    )
}

fn assert_valid(result: &cadmpeg_test_support::EditableDecodeResult) {
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation =
        cadmpeg_ir::validate_neutral(result.ir(), Vec::new()).expect("validate the neutral model");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

fn find_drawing(
    result: &cadmpeg_test_support::EditableDecodeResult,
    native_id: &str,
) -> cadmpeg_ir::drawings::Drawing {
    result
        .ir()
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.native_ref.as_deref() == Some(native_id))
        .unwrap_or_else(|| panic!("{native_id} projects a drawing"))
        .clone()
}

#[test]
fn drawing_sheets_link_their_views_and_views_carry_placed_scale() {
    use cadmpeg_ir::drawings::DrawingKind;

    let result =
        decode(crate::test_support::test_solids_and_structure::drawing_with_properties_file());
    assert_eq!(result.ir().model.drawings.len(), 2);
    let view = find_drawing(&result, "iges:presentation:view#D1");
    assert!(matches!(view.kind, DrawingKind::View));
    assert_eq!(view.order, 0);
    assert_eq!(view.runtime_type, "iges:410");
    assert_eq!(
        view.position,
        cadmpeg_ir::units::FiniteVector::new([10.0, 20.0])
    );
    let rotation = view.rotation_degrees.expect("sheet rotation");
    assert!(
        (rotation.get() - 0.5 * 180.0 / std::f64::consts::PI).abs() < 1.0e-12,
        "{rotation:?}"
    );
    assert_eq!(
        view.scale.map(|scale| scale.get()),
        Some(1.0),
        "{:?}",
        view.scale
    );
    let page = find_drawing(&result, "iges:presentation:drawing#D11");
    assert!(matches!(page.kind, DrawingKind::Page));
    assert_eq!(page.order, 1);
    assert_eq!(page.runtime_type, "iges:404");
    let views = page
        .relationships
        .get(&cadmpeg_core::text::NonBlankString::new("views").expect("views role"));
    assert_eq!(
        views
            .expect("page links its views")
            .iter()
            .map(|selection| selection.target.local())
            .collect::<Vec<_>>(),
        [Some("iges:model:drawing#D1")]
    );
    assert_valid(&result);
}

#[test]
fn unplaced_views_project_without_sheet_positions() {
    use cadmpeg_ir::drawings::DrawingKind;

    let result = decode(crate::test_support::test_solids_and_structure::view_forms_file());
    assert_eq!(result.ir().model.drawings.len(), 3);
    for (index, native_id) in [
        "iges:presentation:view#D1",
        "iges:presentation:view#D3",
        "iges:presentation:view#D5",
    ]
    .into_iter()
    .enumerate()
    {
        let view = find_drawing(&result, native_id);
        assert!(matches!(view.kind, DrawingKind::View));
        assert_eq!(view.order, index as u32);
    }
    assert!(result
        .ir()
        .model
        .drawings
        .iter()
        .all(|drawing| drawing.position.is_none()));
    assert_valid(&result);
}

#[test]
fn segmented_visibility_blocks_project_view_presentations() {
    let result =
        decode(crate::test_support::test_solids_and_structure::segmented_view_visibility_file());
    assert_eq!(result.ir().model.view_presentations.len(), 2);
    let first = &result.ir().model.view_presentations[0];
    assert_eq!(first.order, 0);
    assert_eq!(first.object.as_deref(), Some("iges:presentation:view#D1"));
    assert_eq!(
        first.native_ref.as_deref(),
        Some("iges:presentation:segmented-visibility#D3")
    );
    assert_eq!(
        first
            .properties
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [("breakpoint", "0.5"), ("display_flag", "0")]
    );
    assert_valid(&result);
}

#[test]
fn view_visibility_lists_displayed_entities_on_the_view() {
    let result = decode(owned_test_file(&[
        OwnedTestEntity {
            entity_type: 410,
            form: 0,
            label: "VIEW".into(),
            status: "00000100",
            parameters: "410,1,1,0,0,0,0,0,0,1,5,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "POINT".into(),
            status: "00000100",
            parameters: "116,0,0,0,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 402,
            form: 3,
            label: "VISIBLE".into(),
            status: "00000100",
            parameters: "402,1,1,1,3;".into(),
        },
    ]));
    let view = find_drawing(&result, "iges:presentation:view#D1");
    let displays = view
        .relationships
        .get(&cadmpeg_core::text::NonBlankString::new("displays").expect("displays role"));
    assert_eq!(
        displays
            .expect("view lists its displayed entities")
            .iter()
            .map(|selection| selection.target.local())
            .collect::<Vec<_>>(),
        [Some("iges:entity:directory#3")]
    );
    assert_valid(&result);
}
