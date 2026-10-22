#[test]
fn per_edge_flange_widths_remain_positive_through_mutation_and_serde() {
    use crate::{
        features::{SheetMetalFlangeEdgeWidths, SheetMetalFlangeTwoSidedWidth},
        scalar::PositiveLength,
    };

    let mut widths = SheetMetalFlangeEdgeWidths::new(vec![SheetMetalFlangeTwoSidedWidth {
        first: crate::features::Dimension::Literal(PositiveLength::new(3.0).unwrap()),
        second: crate::features::Dimension::Literal(PositiveLength::new(1.5).unwrap()),
    }])
    .unwrap();
    widths.as_mut_slice()[0].first =
        crate::features::Dimension::Literal(PositiveLength::new(4.0).unwrap());
    widths.as_mut_slice()[0].second =
        crate::features::Dimension::Literal(PositiveLength::new(2.0).unwrap());
    let wire = serde_json::json!([{"first": {"kind":"literal","value":4.0}, "second": {"kind":"literal","value":2.0}}]);
    assert_eq!(serde_json::to_value(&widths).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<SheetMetalFlangeEdgeWidths>(wire.clone()).unwrap(),
        widths
    );
    for field in ["first", "second"] {
        for invalid in [-3.0, 0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(PositiveLength::new(invalid).is_none());
            let mut invalid_wire = wire.clone();
            invalid_wire[0][field]["value"] = serde_json::json!(invalid);
            assert!(
                serde_json::from_value::<SheetMetalFlangeEdgeWidths>(invalid_wire)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
    }
}
