// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    LawExpression, LawFormula, LawFormulaName, LawSurfaceConstruction, LawSurfaceTail,
    ProceduralSurface, ProceduralSurfaceDefinition,
};
use crate::ids::{ProceduralSurfaceId, SurfaceId};

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:procedural_surface#payload").unwrap()
}

fn subset(
    ranges: [[f64; 2]; 2],
) -> Result<ProceduralSurfaceDefinition, crate::geometry::ProceduralGeometryError> {
    Ok(ProceduralSurfaceDefinition::Subset(
        crate::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
            SurfaceId::mint("synthetic:test:surface#support").unwrap(),
            ranges,
            None,
            None,
        )?,
    ))
}

#[test]
fn surface_payload_admission_enforces_directed_nonzero_subset_ranges() {
    let definition = subset([[2.0, -1.0], [0.0, 1.0]]).unwrap();
    let surface = ProceduralSurface::new(id(), definition.clone(), None).unwrap();
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(
        wire["parameter_ranges"],
        serde_json::json!([[2.0, -1.0], [0.0, 1.0]])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire).unwrap(),
        definition
    );
    let before = surface.clone();
    for range in [[0.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(subset([range, [0.0, 1.0]]).is_err());
        let mut invalid = serde_json::to_value(&before).unwrap();
        invalid["definition"]["parameter_ranges"][0] = serde_json::json!(range);
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(
            invalid["definition"].clone()
        )
        .is_err());
        assert!(serde_json::from_value::<ProceduralSurface>(invalid).is_err());
    }
    let mut wire = serde_json::to_value(&before).unwrap();
    wire["definition"]["parameter_ranges"][0] = serde_json::json!([1.0, 1.0]);
    assert!(
        serde_json::from_value::<ProceduralSurfaceDefinition>(wire["definition"].clone()).is_err()
    );
    assert!(serde_json::from_value::<ProceduralSurface>(wire).is_err());
}

#[test]
fn surface_law_admission_preserves_the_depth_boundary() {
    let mut expression = LawExpression::Null;
    for _ in 0..64 {
        expression = LawExpression::Algebraic {
            operator: "+".into(),
            operands: vec![expression],
        };
    }
    let construction = |expression| LawSurfaceConstruction {
        parameter_ranges: None,
        primary: LawFormula::Named {
            name: LawFormulaName::new("test").unwrap(),
            variables: vec![expression],
        },
        additional: Vec::new(),
        tail: LawSurfaceTail::Historical,
        discontinuities: std::array::from_fn(|_| Vec::new()),
    };
    let law = |expression| {
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(construction(
            expression,
        )))
        .map(ProceduralSurfaceDefinition::Law)
    };
    assert!(ProceduralSurface::new(id(), law(expression.clone()).unwrap(), None).is_ok());
    expression = LawExpression::Algebraic {
        operator: "+".into(),
        operands: vec![expression],
    };
    assert!(law(expression).is_err());
    let invalid = LawExpression::Text {
        value: String::new(),
    };
    assert!(law(invalid.clone()).is_err());
    let wire = serde_json::json!({"kind": "law", "construction": construction(invalid)});
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(wire).is_err());
    assert!(ProceduralSurface::new(
        id(),
        law(LawExpression::Text { value: " ".into() }).unwrap(),
        None
    )
    .is_ok());
}

#[test]
fn linear_sweep_admission_requires_a_finite_nondegenerate_direction() {
    use crate::geometry::surface_payloads::LinearSweepSurfaceConstruction;
    use crate::ids::CurveId;
    use crate::math::Vector3;

    let directrix = CurveId::mint("synthetic:test:curve#directrix").unwrap();
    let sweep = |direction| {
        LinearSweepSurfaceConstruction::try_new(directrix.clone(), direction)
            .map(ProceduralSurfaceDefinition::LinearSweep)
    };
    let valid = sweep(Vector3::new(0.0, 0.0, 2.0)).unwrap();
    let surface = ProceduralSurface::new(id(), valid.clone(), None).unwrap();
    let wire = serde_json::to_value(&surface).unwrap();
    assert_eq!(
        wire["definition"]["direction"],
        serde_json::json!({"x": 0.0, "y": 0.0, "z": 2.0})
    );
    assert_eq!(
        serde_json::from_value::<ProceduralSurface>(wire.clone()).unwrap(),
        surface
    );
    for direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 0.0, 1.0),
        Vector3::new(0.0, f64::INFINITY, 0.0),
    ] {
        assert!(sweep(direction).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["direction"] = serde_json::to_value(direction).unwrap();
        assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(
            invalid["definition"].clone()
        )
        .is_err());
        assert!(serde_json::from_value::<ProceduralSurface>(invalid).is_err());
    }
}

#[test]
fn rejected_surface_definition_changes_preserve_serialized_owner() {
    let mut surface = ProceduralSurface::try_new(
        id(),
        subset([[0.0, 1.0], [0.0, 1.0]]).unwrap(),
        Some(0.5),
        None,
    )
    .unwrap();
    let before = serde_json::to_vec(&surface).unwrap();
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(surface
            .try_replace_definition(subset([[2.0, 3.0], [4.0, 5.0]]).unwrap(), Some(tolerance),)
            .is_err());
        assert_eq!(serde_json::to_vec(&surface).unwrap(), before);
    }
    let incompatible = ProceduralSurfaceDefinition::Law(
        crate::geometry::surface_payloads::LawSurfacePayload::try_new(Box::new(
            LawSurfaceConstruction {
                parameter_ranges: None,
                primary: LawFormula::Null,
                additional: Vec::new(),
                tail: LawSurfaceTail::Historical,
                discontinuities: std::array::from_fn(|_| Vec::new()),
            },
        ))
        .unwrap(),
    );
    assert!(surface
        .edit_definition(|definition| *definition = incompatible)
        .is_err());
    assert_eq!(serde_json::to_vec(&surface).unwrap(), before);
}
