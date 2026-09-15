// SPDX-License-Identifier: Apache-2.0

use crate::assets::{Asset, AssetContent, AssetData};
use crate::document::{CadIr, Model};
use crate::features::{Feature, FeatureDefinition, FeatureOperation};
use crate::math::Point3;
use crate::native::{Native, NativeRecord};
use crate::topology::Point;

fn staged_document() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.points.push(Point {
        id: "test:append:point#new".try_into().unwrap(),
        position: Point3::new(1.0, 2.0, 3.0),
        source_object: None,
    });
    ir.model.assets.push(Asset {
        id: "test:append:asset#new".try_into().unwrap(),
        name: None,
        media_type: None,
        content: AssetContent::Embedded {
            data: AssetData::new(vec![1, 2, 3]).unwrap(),
        },
        native_ref: None,
    });
    for (ordinal, key) in ["parent", "child"].into_iter().enumerate() {
        ir.model.features.push(Feature::new(
            format!("test:append:feature#{key}").try_into().unwrap(),
            ordinal as u64,
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
        ));
    }
    ir.model
        .set_feature_regeneration_parent(
            "test:append:feature#child".try_into().unwrap(),
            "test:append:feature#parent".try_into().unwrap(),
        )
        .unwrap();
    for (format, arena, key) in [
        ("test", "existing", "appended"),
        ("test", "new", "new-arena"),
        ("other", "new", "new-namespace"),
    ] {
        ir.native.namespace_mut(format).arenas_mut().insert(
            arena.into(),
            vec![
                NativeRecord::new(format!("test:append:record#{key}"), serde_json::Map::new())
                    .unwrap(),
            ],
        );
    }
    // Exercise the complete-document admission route before the append route.
    CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap()
}

#[test]
fn rejected_append_restores_neutral_native_and_parent_state() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "existing".into(),
        vec![NativeRecord::new("test:append:record#original", serde_json::Map::new()).unwrap()],
    );
    ir.native.namespace_mut("empty");
    let before = ir.clone();
    let staged = staged_document();
    let result = ir.try_append(staged.model, staged.native, |combined| {
        assert_eq!(combined.model.points.len(), before.model.points.len() + 1);
        assert_eq!(combined.model.assets.len(), 1);
        assert!(combined
            .model
            .feature_regeneration_parent(&"test:append:feature#child".try_into().unwrap())
            .is_some());
        assert_eq!(
            combined.native.namespace("test").unwrap().arenas()["existing"].len(),
            2
        );
        assert!(combined.native.namespace("other").is_some());
        Err::<(), _>("independent rejection")
    });
    assert_eq!(result, Err("independent rejection"));
    assert_eq!(ir, before);
}

#[test]
fn accepted_append_preserves_all_staged_records_and_parent_relations() {
    let mut ir = CadIr::empty();
    let staged = staged_document();
    ir.try_append(staged.model.clone(), staged.native.clone(), |_| {
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(ir.model, staged.model);
    assert_eq!(ir.native, staged.native);
    assert_eq!(
        CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap(),
        ir
    );
}

#[test]
fn actual_admission_rejects_duplicate_identity_without_changing_the_document() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let before = ir.clone();
    let model = Model {
        points: vec![ir.model.points[0].clone()],
        ..Model::default()
    };
    let result = ir.try_append(model, Native::default(), |combined| {
        let report = crate::admit(combined, crate::DRAFT_CORE_CHECKS, Vec::new());
        if report.is_ok() {
            Ok(())
        } else {
            Err(report)
        }
    });
    assert!(result.is_err());
    assert_eq!(ir, before);
}
