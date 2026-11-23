use super::semantic_root;
use crate::swift::pmi_id;
use crate::swift::project;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::design::parameter::Tolerance;
use cadmpeg_ir::pmi::{PmiAnnotation, PmiDefinition};

fn diameter(annotations: &[PmiAnnotation]) -> (&PmiAnnotation, Option<&Tolerance>) {
    let annotation = annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id("A30").unwrap())
        .expect("diameter annotation");
    let PmiDefinition::Dimension(dimension) = &annotation.definition else {
        panic!("dimension definition");
    };
    (annotation, dimension.tolerance.as_ref())
}

#[test]
fn equal_deviations_state_no_tolerance_and_keep_the_stored_pair() {
    let root = semantic_root();
    let annotations = project(&root);
    let (annotation, tolerance) = diameter(&annotations);
    assert!(
        matches!(tolerance, Some(Tolerance::Deviations(deviations))
            if deviations.lower.get() == -0.1 && deviations.upper.get() == 0.2),
        "{tolerance:?}"
    );
    assert!(annotation.native_options.is_empty());

    let mut root = semantic_root();
    let entity = root.annotations.entities.get_mut(2).expect("diameter");
    entity.doubles.insert("MinusTolerance".into(), 0.0);
    entity.doubles.insert("PlusTolerance".into(), 0.0);
    let annotations = project(&root);
    let (annotation, tolerance) = diameter(&annotations);
    assert!(tolerance.is_none(), "{tolerance:?}");
    for key in ["sldprt:lower_deviation", "sldprt:upper_deviation"] {
        assert!(
            matches!(annotation.native_options.get(key),
                Some(NativeOption::Length(Dimension::Literal(value))) if value.get() == 0.0),
            "{key}: {:?}",
            annotation.native_options
        );
    }
}
