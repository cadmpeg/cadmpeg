// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::selections::{GeneratedFaceRef, NonEmptyMembers, SelectionReference};
use cadmpeg_ir::features::FeatureId;

pub(super) fn construction() -> DesignThreadConstruction {
    cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "form": "standard_extended", "designation_offset": 100,
        "designation": "M10x1.5", "nominal_size_text": "10", "nominal_size": 10.0,
        "profile": "ISO Metric profile", "major_diameter": 1.0,
        "minor_diameter": 0.8, "pitch_diameter": 0.9, "pitch": 0.15,
        "modeled": true, "face_group_record_indices": [10],
        "extended": {"class": "6g", "external": true,
            "full_length": true, "right_handed": false}
    }))
    .unwrap()
}

#[test]
fn thread_spec_carries_class_hand_and_physical_diameters() {
    let construction = construction();
    let projected = spec(&construction, Some(&CosmeticThreadExtent::Through {}));
    let standard = projected.standard.unwrap();
    assert_eq!(standard.standard.as_str(), "ISO Metric profile");
    assert_eq!(
        standard.size,
        Some(Dimension::Literal(construction.designation))
    );
    assert_eq!(
        standard.class.as_ref().map(NonBlankString::as_str),
        Some("6g")
    );
    assert_eq!(projected.hand, Some(Hand::Left));
    for (actual, millimetres) in [
        (projected.major_diameter, 10.0),
        (projected.minor_diameter, 8.0),
        (projected.pitch_diameter, 9.0),
        (projected.pitch, 1.5),
    ] {
        assert_eq!(
            actual,
            PositiveLength::new(millimetres).map(Dimension::Literal)
        );
    }
    assert_eq!(projected.modeled, Some(Dimension::Literal(true)));
    assert_eq!(projected.length, Some(ThreadLength::Full));
}

#[test]
fn absent_extended_fields_stay_absent_and_blind_length_stays_literal() {
    let mut construction = construction();
    construction.extended = None;
    let length = PositiveLength::new(12.0).unwrap();
    let projected = spec(&construction, Some(&CosmeticThreadExtent::Blind { length }));
    assert_eq!(projected.hand, None);
    assert_eq!(projected.standard.unwrap().class, None);
    assert_eq!(
        projected.length,
        Some(ThreadLength::Blind(Dimension::Literal(length)))
    );
    assert_eq!(spec(&construction, None).length, None);
}

#[test]
fn compact_thread_class_comes_from_its_fixed_prefix() {
    let mut construction = construction();
    construction.extended = None;
    for form in [
        crate::records::feature::thread::DesignThreadForm::Compact(None),
        crate::records::feature::thread::DesignThreadForm::CompactOwnersLeading,
        crate::records::feature::thread::DesignThreadForm::CompactOwnersTrailing,
    ] {
        construction.form = form;
        let projected = spec(&construction, None);
        assert_eq!(
            projected
                .standard
                .unwrap()
                .class
                .as_ref()
                .map(NonBlankString::as_str),
            Some("6H")
        );
        assert_eq!(projected.hand, None);
    }
}

#[test]
fn thread_selection_without_a_native_face_reference_stays_unresolved() {
    let generated = FaceSelection::Generated {
        faces: NonEmptyMembers::one(GeneratedFaceRef {
            feature: FeatureId::mint("f3d:model:feature#thread").unwrap(),
            local_id: SelectionReference::new("face#1".to_owned()).unwrap(),
        }),
        native: SelectionReference::new("f3d:test:face-selection#10".to_owned()).unwrap(),
    };
    for face in [
        FaceSelection::Unresolved,
        FaceSelection::Faces(Vec::new()),
        generated,
    ] {
        assert!(selection(&face).is_none());
    }
}

#[test]
fn thread_selection_keeps_its_native_face_reference() {
    let native = "f3d:test:design-construction-operand-group#10";
    let face = selection(&FaceSelection::Native(native.to_owned())).unwrap();
    assert_eq!(face.at, Anchor::Input);
    assert!(matches!(face.picks, Picks::Unread));
    assert_eq!(
        face.native,
        Some(NativeSelections::try_from(vec![native.to_owned()]).unwrap())
    );
}

#[test]
fn standard_thread_class_comes_from_its_fixed_prefix() {
    let mut construction = construction();
    construction.extended = None;
    for form in [
        crate::records::feature::thread::DesignThreadForm::Standard,
        crate::records::feature::thread::DesignThreadForm::StandardLegacy,
        crate::records::feature::thread::DesignThreadForm::StandardCompact,
    ] {
        construction.form = form;
        let projected = spec(&construction, None);
        assert_eq!(
            projected
                .standard
                .unwrap()
                .class
                .as_ref()
                .map(NonBlankString::as_str),
            Some("6g")
        );
    }
}

#[test]
fn an_unpaired_thread_item_is_found_by_its_scope_record_item() {
    use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
    let feature = |id: &FeatureId, family| Feature {
        id: id.clone(),
        name: None,
        suppressed: None,
        definition: cadmpeg_ir::design::feature::FeatureDefinition {
            operation: FeatureOperation::Unresolved { family },
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    };
    let id = |record| {
        crate::design_v11::timeline::item_feature_id("f3d:Design/BulkStream.dat", record).unwrap()
    };
    let (other, legacy, item) = (id(1), id(2), id(3));
    let mut features = [
        feature(&other, OperationFamily::Thread),
        feature(&item, OperationFamily::Thread),
    ];
    assert_eq!(
        thread_target(features.iter_mut(), &legacy, Some(&item)).map(|found| found.id.clone()),
        Some(item.clone())
    );
    assert!(thread_target(features.iter_mut(), &legacy, None).is_none());
    features[1] = feature(&item, OperationFamily::Sketch);
    assert!(thread_target(features.iter_mut(), &legacy, Some(&item)).is_none());
    features[0] = feature(&legacy, OperationFamily::Sketch);
    assert!(thread_target(features.iter_mut(), &legacy, None).is_some());
}
