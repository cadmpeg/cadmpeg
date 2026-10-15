// SPDX-License-Identifier: Apache-2.0
use super::neutral_configuration_id;

#[test]
fn configuration_identifier_preserves_encoded_bytes() {
    for entry in ["plain.dsgcfg", "asset/a:#% b\u{2003}ç.dsgcfg"] {
        for name in ["Small", "v:#%\u{a0}ç"] {
            assert_eq!(
                neutral_configuration_id(entry, name).unwrap(),
                crate::ids::neutral_configuration_id(entry, name)
            );
        }
    }
}

fn encoded_scope() -> crate::records::feature::scope::DesignParameterScope {
    crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:asset/a %\u{2003}ç:parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::try_from("Native:#% \u{a0}ç".to_owned())
            .unwrap(),
        42,
    )
}

fn encoded_placement() -> crate::records::sketch_placement::DesignSketchPlacement {
    crate::records::sketch_placement::DesignSketchPlacement {
        id: "f3d:asset/a %\u{2003}ç:design-sketch-placement#0".to_owned(),
        scope_record_index: Some(42),
        entity_id: "sketch_100".to_owned().try_into().unwrap(),
        visibility: None,
        class_tag: "356".to_owned().try_into().unwrap(),
        record_index: 43,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
    }
}

#[test]
fn neutral_feature_id_preserves_bytes() {
    let scope = encoded_scope();
    assert_eq!(
        super::neutral_feature_id(&scope).unwrap(),
        crate::ids::neutral_feature_id(&scope)
    );
}

#[test]
fn neutral_component_insert_occurrence_id_preserves_bytes() {
    let scope = encoded_scope();
    assert_eq!(
        super::neutral_component_insert_occurrence_id(&scope).unwrap(),
        crate::ids::neutral_component_insert_occurrence_id(&scope)
    );
}

#[test]
fn neutral_sketch_id_preserves_bytes() {
    let placement = encoded_placement();
    assert_eq!(
        super::neutral_sketch_id(&placement).unwrap(),
        crate::ids::neutral_sketch_id(&placement)
    );
}

#[test]
fn neutral_spatial_sketch_id_preserves_bytes() {
    let placement = encoded_placement();
    assert_eq!(
        super::neutral_spatial_sketch_id(&placement).unwrap(),
        crate::ids::neutral_spatial_sketch_id(&placement)
    );
}

#[test]
fn neutral_parameter_id_preserves_bytes() {
    let parameter = crate::design::decode::parameters::parse_design_parameter_record(
        &crate::design::test_support::parameter_record(
            Some(40),
            "1 cm",
            "AlongDistance",
            Some("cm"),
            "d9",
            1.0,
        ),
    )
    .unwrap();
    assert_eq!(
        super::neutral_parameter_id(&parameter).unwrap(),
        crate::ids::neutral_parameter_id(&parameter)
    );
}

#[test]
fn neutral_sketch_point_id_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_sketch_point_id(&sketch, u64::MAX).unwrap(),
        crate::ids::neutral_sketch_point_id(&sketch, u64::MAX)
    );
}

#[test]
fn neutral_sketch_record_id_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_sketch_record_id(&sketch, u32::MAX).unwrap(),
        crate::ids::neutral_sketch_record_id(&sketch, u32::MAX)
    );
}

#[test]
fn neutral_sketch_text_id_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_sketch_text_id(&sketch, u64::MAX).unwrap(),
        crate::ids::neutral_sketch_text_id(&sketch, u64::MAX)
    );
}

#[test]
fn neutral_sketch_curve_id_preserves_bytes() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_sketch_curve_id(&sketch, u64::MAX, u64::MAX).unwrap(),
        crate::ids::neutral_sketch_curve_id(&sketch, u64::MAX, u64::MAX)
    );
}

#[test]
fn neutral_spatial_sketch_point_id_preserves_bytes() {
    let sketch =
        cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_spatial_sketch_point_id(&sketch, u64::MAX).unwrap(),
        crate::ids::neutral_spatial_sketch_point_id(&sketch, u64::MAX)
    );
}

#[test]
fn neutral_spatial_sketch_record_id_preserves_bytes() {
    let sketch =
        cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_spatial_sketch_record_id(&sketch, u32::MAX).unwrap(),
        crate::ids::neutral_spatial_sketch_record_id(&sketch, u32::MAX)
    );
}

#[test]
fn neutral_spatial_sketch_surface_id_preserves_bytes() {
    let sketch =
        cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_spatial_sketch_surface_id(&sketch, u64::MAX).unwrap(),
        crate::ids::neutral_spatial_sketch_surface_id(&sketch, u64::MAX)
    );
}

#[test]
fn neutral_spatial_sketch_curve_id_preserves_bytes() {
    let sketch =
        cadmpeg_ir::sketches::SpatialSketchId::mint("test:model:sketch#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_spatial_sketch_curve_id(&sketch, u64::MAX, u64::MAX).unwrap(),
        crate::ids::neutral_spatial_sketch_curve_id(&sketch, u64::MAX, u64::MAX)
    );
}

#[test]
fn neutral_sketch_constraint_id_preserves_bytes() {
    let native = "f3d:asset/a %\u{2003}ç:relation#42";
    assert_eq!(
        super::neutral_sketch_constraint_id(native, u32::MAX).unwrap(),
        crate::ids::neutral_sketch_constraint_id(native, u32::MAX)
    );
}

#[test]
fn neutral_dimension_constraint_id_preserves_bytes() {
    let parameter =
        cadmpeg_ir::features::ParameterId::mint("test:model:parameter#encoded%20ç").unwrap();
    assert_eq!(
        super::neutral_dimension_constraint_id(&parameter, "a:#% \u{2003}ç").unwrap(),
        crate::ids::neutral_dimension_constraint_id(&parameter, "a:#% \u{2003}ç")
    );
}

#[test]
fn configuration_entry_id_preserves_bytes() {
    let entry = "asset/a:#% \u{2003}ç.dsgcfg";
    assert_eq!(
        super::configuration_entry_id(entry),
        crate::ids::configuration_entry_id(
            entry,
            &cadmpeg_ir::identity_component!("configuration"),
        )
    );
}
