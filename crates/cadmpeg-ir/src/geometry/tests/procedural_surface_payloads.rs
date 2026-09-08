// SPDX-License-Identifier: Apache-2.0
use crate::geometry::{
    LawExpression, LawFormula, LawFormulaName, LawSurfaceConstruction, LawSurfaceTail,
    ProceduralSurface, ProceduralSurfaceDefinition,
};
use crate::ids::{ProceduralSurfaceId, SurfaceId};

fn id() -> ProceduralSurfaceId {
    ProceduralSurfaceId::mint("synthetic:test:procedural_surface#payload").unwrap()
}

fn subset(ranges: [[f64; 2]; 2]) -> ProceduralSurfaceDefinition {
    ProceduralSurfaceDefinition::Subset {
        support: SurfaceId::mint("synthetic:test:surface#support").unwrap(),
        parameter_ranges: ranges,
        u_sense: None,
        v_sense: None,
    }
}

#[test]
fn surface_payload_admission_and_edits_enforce_directed_nonzero_subset_ranges() {
    let definition = subset([[2.0, -1.0], [0.0, 1.0]]);
    let mut surface = ProceduralSurface::new(id(), definition.clone(), None).unwrap();
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
        let invalid = subset([range, [0.0, 1.0]]);
        assert!(ProceduralSurface::new(id(), invalid.clone(), None).is_err());
        assert!(surface.replace_definition(invalid.clone()).is_err());
        assert_eq!(surface, before);
        assert!(surface
            .try_replace_definition(invalid.clone(), Some(2.0))
            .is_err());
        assert_eq!(surface, before);
        assert!(surface
            .edit_definition(|definition| *definition = invalid)
            .is_err());
        assert_eq!(surface, before);
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
    let law = |expression| ProceduralSurfaceDefinition::Law {
        construction: Box::new(LawSurfaceConstruction {
            parameter_ranges: None,
            primary: LawFormula::Named {
                name: LawFormulaName::new("test").unwrap(),
                variables: vec![expression],
            },
            additional: Vec::new(),
            tail: LawSurfaceTail::Historical,
            discontinuities: std::array::from_fn(|_| Vec::new()),
        }),
    };
    assert!(ProceduralSurface::new(id(), law(expression.clone()), None).is_ok());
    expression = LawExpression::Algebraic {
        operator: "+".into(),
        operands: vec![expression],
    };
    assert!(ProceduralSurface::new(id(), law(expression), None).is_err());
    let invalid = law(LawExpression::Text {
        value: String::new(),
    });
    assert!(ProceduralSurface::new(id(), invalid.clone(), None).is_err());
    let wire = serde_json::to_value(invalid).unwrap();
    assert!(serde_json::from_value::<ProceduralSurfaceDefinition>(wire).is_err());
    assert!(
        ProceduralSurface::new(id(), law(LawExpression::Text { value: " ".into() }), None).is_ok()
    );
}
