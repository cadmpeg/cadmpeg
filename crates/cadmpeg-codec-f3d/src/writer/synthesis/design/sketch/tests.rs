// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::sketches::SketchConstraint;

use super::matching_constraint;

fn source_constraint() -> SketchConstraint {
    let source =
        include_bytes!("../../../../../tests/golden/fixtures/design_sketch_constraints.f3d");
    let decoded = crate::F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("decode the unchanged dimensional fixture");
    decoded.ir().model.sketch_constraints[0].clone()
}

fn alias(source: &SketchConstraint, name: &str) -> SketchConstraint {
    let mut candidate = source.clone();
    candidate.id = format!("f3d:model:sketch-constraint#{name}")
        .try_into()
        .expect("valid independent projected identity");
    candidate.native_ref = None;
    candidate
}

#[test]
fn a_unique_carrier_alias_preserves_all_canonical_constraint_fields() {
    let source = source_constraint();
    let candidate = alias(&source, "alias");
    let projected = [candidate];
    let mut consumed = HashSet::new();
    let matched = matching_constraint(&source, &projected, &consumed)
        .expect("a unique full-content alias matches");
    assert_eq!(matched.id, projected[0].id);
    consumed.insert(matched.id.clone());
    assert!(matching_constraint(&source, &projected, &consumed).is_none());
    let mut changed = alias(&source, "changed");
    changed.visible = Some(true);
    assert!(matching_constraint(&source, &[changed], &HashSet::new()).is_none());
}

#[test]
fn duplicate_aliases_are_ambiguous_but_an_exact_identity_takes_precedence() {
    let source = source_constraint();
    let projected = [alias(&source, "first"), alias(&source, "second")];
    assert!(matching_constraint(&source, &projected, &HashSet::new()).is_none());
    let projected = [projected[0].clone(), source.clone(), projected[1].clone()];
    assert_eq!(
        matching_constraint(&source, &projected, &HashSet::new())
            .expect("the exact identity remains authoritative")
            .id,
        source.id
    );
}
