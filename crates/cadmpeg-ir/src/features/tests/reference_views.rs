// SPDX-License-Identifier: Apache-2.0

use crate::features::{
    DatumPlaneReference, DatumPointConstruction, FaceSelection, FeatureId, PlanarProfileRef,
    SolidSweepOperation, SweepMode, SweepSection, SweepShape,
};
use crate::sketches::SketchId;

#[test]
fn datum_point_reference_view_preserves_borrowed_order_and_duplicates() {
    let id = FeatureId::mint("test:model:feature#plane").unwrap();
    let construction = DatumPointConstruction::ThreePlaneIntersection {
        planes: Box::new([
            DatumPlaneReference::Feature {
                feature: id.clone(),
            },
            DatumPlaneReference::Face {
                face: FaceSelection::Unresolved,
            },
            DatumPlaneReference::Feature { feature: id },
        ]),
    };
    let DatumPointConstruction::ThreePlaneIntersection { planes } = &construction else {
        panic!("plane fixture");
    };
    let DatumPlaneReference::Feature { feature: first } = &planes[0] else {
        panic!("first feature");
    };
    let DatumPlaneReference::Feature { feature: last } = &planes[2] else {
        panic!("last feature");
    };
    let mut references = construction.feature_references();
    assert!(std::ptr::eq(references.next().unwrap(), first));
    assert!(std::ptr::eq(references.next().unwrap(), last));
    assert!(references.next().is_none());
}

#[test]
fn sweep_profile_reference_view_borrows_in_primary_and_additional_order() {
    for mode in [
        SweepMode::Unresolved {},
        SweepMode::Surface {},
        SweepMode::Solid {
            op: SolidSweepOperation::NewBody,
        },
    ] {
        let primary =
            PlanarProfileRef::Sketch(SketchId::mint("test:model:sketch#primary").unwrap());
        let additional =
            PlanarProfileRef::Sketch(SketchId::mint("test:model:sketch#additional").unwrap());
        let shape = SweepShape::sheet_sections(
            mode,
            SweepSection::Profile(primary.clone()),
            vec![
                SweepSection::Unresolved(None),
                SweepSection::Profile(additional.clone()),
            ],
        );
        let mut references = shape.referenced_profiles();
        let first = references.next().unwrap();
        assert_eq!(first, &primary);
        assert!(std::ptr::eq(first, shape.referenced_profile().unwrap()));
        let last = references.next().unwrap();
        assert_eq!(last, &additional);
        assert!(references.next().is_none());
        let mut cloned = shape.referenced_profiles().clone();
        assert!(std::ptr::eq(cloned.next().unwrap(), first));
        assert!(std::ptr::eq(cloned.next().unwrap(), last));
        assert!(cloned.next().is_none());
    }
}
