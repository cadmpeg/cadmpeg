//! Tests for the `relation_loci` module.

use super::unique_locus;
use cadmpeg_ir::sketches::{SketchEntityId, SketchLocus};

#[test]
fn point_operand_requires_one_profile_locus() {
    let entity = SketchEntityId("entity".into());
    let locus = SketchLocus::Start(entity.clone());
    assert_eq!(unique_locus(std::slice::from_ref(&locus)), Some(locus));
    assert_eq!(unique_locus(&[]), None);
    assert_eq!(
        unique_locus(&[SketchLocus::Start(entity.clone()), SketchLocus::End(entity)]),
        None
    );
}
