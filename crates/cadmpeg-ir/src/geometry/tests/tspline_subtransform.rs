use super::super::{InlineTSplineSubtransform, SubtypeTableIndex, TSplineSubtransform};
use serde_json::json;

#[test]
fn subtransform_admission_requires_nonempty_programs_and_nonnegative_indices() {
    assert!(InlineTSplineSubtransform::try_new("", None, "values").is_err());
    assert!(InlineTSplineSubtransform::try_new("program", None, "").is_err());
    assert!(InlineTSplineSubtransform::try_new(" ", None, "\n").is_err());
    assert!(InlineTSplineSubtransform::try_new(" p ", None, " v\n").is_ok());
    assert!(SubtypeTableIndex::try_new(-1).is_err());
    for index in [0, i64::MAX] {
        assert_eq!(SubtypeTableIndex::try_new(index).unwrap().get(), index);
    }
}

#[test]
fn subtransform_wire_preserves_inline_and_reference_shapes() {
    let inline =
        json!({"kind": "inline", "program": "program", "separator": true, "values": "values"});
    for wire in [
        inline.clone(),
        json!({"kind": "reference", "index": 4, "resolved": inline}),
    ] {
        let value: TSplineSubtransform = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
}

#[test]
fn subtransform_wire_rejects_invalid_payloads_and_recursive_references() {
    for wire in [
        json!({"kind": "inline", "program": "", "values": "values"}),
        json!({"kind": "inline", "program": "program", "values": ""}),
        json!({"kind": "inline", "values": "values"}),
        json!({"kind": "reference", "index": -1}),
        json!({"kind": "reference", "index": 0, "resolved": {"kind": "reference", "index": 1}}),
        json!({"kind": "reference", "index": 0, "resolved": {"kind": "inline", "program": "", "values": "values"}}),
    ] {
        assert!(serde_json::from_value::<TSplineSubtransform>(wire).is_err());
    }
}

#[test]
fn surface_admission_requires_ordered_ranges_and_resolved_subtransform() {
    use super::super::TSplineSurfaceConstruction;

    let inline = TSplineSubtransform::Inline(
        InlineTSplineSubtransform::try_new("program", None, "values").unwrap(),
    );
    let admit = |ranges, subtransform| {
        TSplineSurfaceConstruction::try_new(
            ranges,
            0,
            subtransform,
            0,
            Default::default(),
            false,
            None,
        )
    };
    let valid = admit([[0.0, 1.0], [2.0, 2.0]], inline.clone()).unwrap();
    assert_eq!(valid.parameter_ranges(), [[0.0, 1.0], [2.0, 2.0]]);
    for ranges in [
        [[1.0, 0.0], [0.0, 1.0]],
        [[0.0, 1.0], [1.0, 0.0]],
        [[f64::NAN, 1.0], [0.0, 1.0]],
        [[0.0, 1.0], [0.0, f64::INFINITY]],
    ] {
        assert!(admit(ranges, inline.clone()).is_err());
    }
    let wire = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<TSplineSurfaceConstruction>(wire.clone()).unwrap(),
        valid
    );
    let mut reversed = wire.clone();
    reversed["parameter_ranges"] = json!([[1.0, 0.0], [0.0, 1.0]]);
    assert!(serde_json::from_value::<TSplineSurfaceConstruction>(reversed).is_err());
    let mut unresolved = wire;
    unresolved["subtransform"] = json!({"kind": "reference", "index": 0});
    assert!(serde_json::from_value::<TSplineSurfaceConstruction>(unresolved).is_err());
}

#[test]
fn subtransform_wire_rejects_missing_resolved_payload() {
    for wire in [
        json!({"kind": "reference", "index": 0}),
        json!({"kind": "reference", "index": 0, "resolved": null}),
    ] {
        serde_json::from_value::<TSplineSubtransform>(wire)
            .expect_err("a reference states its resolved program");
    }
}

/// The two program graphs are parsed from the subtransform, so the wire states
/// them nowhere and cannot disagree with them.
#[test]
fn a_tspline_construction_states_no_program_graph_on_its_wire() {
    use super::super::TSplineSurfaceConstruction;

    let inline = TSplineSubtransform::Inline(
        InlineTSplineSubtransform::try_new("v 1 2", None, "e 3").unwrap(),
    );
    let construction = TSplineSurfaceConstruction::try_new(
        [[0.0, 1.0], [0.0, 1.0]],
        7,
        inline,
        4,
        Default::default(),
        false,
        None,
    )
    .unwrap();
    assert_eq!(construction.program_graph().records().len(), 1);

    let wire = serde_json::to_value(&construction).unwrap();
    assert!(wire.get("program_graph").is_none());
    assert!(wire.get("values_graph").is_none());
    assert_eq!(
        serde_json::from_value::<TSplineSurfaceConstruction>(wire.clone()).unwrap(),
        construction
    );

    let mut restated = wire;
    restated.as_object_mut().unwrap().insert(
        "program_graph".to_string(),
        json!({"headers": [], "records": [], "unparsed_lines": []}),
    );
    let error = serde_json::from_value::<TSplineSurfaceConstruction>(restated)
        .unwrap_err()
        .to_string();
    assert!(error.contains("program_graph"), "{error}");
}
