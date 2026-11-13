use super::*;
use crate::design::decode::scopes::position::decode;
use crate::test_support::position::{fixture, OCCURRENCE_KEY};

fn scope(matrices: &[Option<[[f64; 4]; 4]>]) -> DesignParameterScope {
    let (bytes, mut scope, types) = fixture(matrices);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    scope.captured_positions = crate::test_support::with_decode_context(|ctx| {
        decode(ctx, &bytes, &records, &scope, &types).unwrap()
    });
    scope
}

fn feature(scope: &DesignParameterScope) -> Feature {
    serde_json::from_value(serde_json::json!({
        "id": "synthetic:test:feature#position", "ordinal": 0, "suppressed": false,
        "native_ref": scope.id, "definition": project(scope, &mut Vec::new()).unwrap(),
    }))
    .unwrap()
}

#[test]
fn position_projects_ordered_affine_matrices_and_named_assumptions() {
    let matrix = [
        [-2.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, -3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let scope = scope(&[Some(matrix), None]);
    let mut losses = Vec::new();
    let FeatureDefinition::Operation(FeatureOperation::CapturedComponentPositions { positions }) =
        project(&scope, &mut losses).unwrap()
    else {
        panic!("captured positions")
    };
    assert_eq!(positions.len(), 2);
    assert_eq!(
        positions[0].selector.as_str(),
        "f3d:Design/BulkStream.dat:design-record#700"
    );
    assert_eq!(
        positions[0].transform,
        CapturedPositionTransform::Affine {
            transform: Transform::affine([
                [-2.0, 0.0, 0.0, 20.0],
                [0.0, 1.0, 0.0, -30.0],
                [0.0, 0.0, 1.0, 40.0],
            ])
            .unwrap()
        }
    );
    assert_eq!(
        positions[1].transform,
        CapturedPositionTransform::Affine {
            transform: Transform::identity()
        }
    );
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.to_string())
            .collect::<Vec<_>>(),
        [
            "f3d/feature.position-semantics-assumed",
            "f3d/feature.position-identity-assumed"
        ]
    );
    assert!(losses[0].message.contains("assembly context"));
    assert!(losses[0].message.contains("frame remain inferred"));
}

#[test]
fn position_resolves_unique_complete_paths_despite_ambiguous_prefixes_and_the_converse() {
    const CHILD: &str = "22222222-3333-4444-8555-666666666666";
    let occurrence = cadmpeg_ir::products::Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId::mint("synthetic:test:occurrence#one").unwrap(),
        prototype: cadmpeg_ir::products::PrototypeReference::Local {
            definition: cadmpeg_ir::ids::ProductDefinitionId::mint("synthetic:test:definition#one")
                .unwrap(),
        },
        parent: cadmpeg_ir::products::OccurrenceParent::Root {},
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: Some("f3d:Design/BulkStream.dat:occurrence#key".into()),
    };
    let carrier = crate::records::feature::assembly_features::DesignComponentOccurrence::try_new(
        crate::records::feature::assembly_features::DesignComponentOccurrenceDraft {
            id: occurrence.native_ref.clone().unwrap(), class_tag: "129".to_owned().try_into().unwrap(),
            record_index: 800, byte_offset: 0, component_record_index: 900,
            component_guid: OCCURRENCE_KEY.to_owned().try_into().unwrap(),
            occurrence_guid: OCCURRENCE_KEY.to_owned().try_into().unwrap(),
            placement: crate::records::feature::assembly_features::DesignComponentOccurrencePlacement::Base,
            owner_entity: 0,
        }
    ).unwrap();
    let (mut bytes, mut complete_scope, mut types) = fixture(&[None]);
    crate::test_support::position::append_locator_path(
        &mut bytes,
        &mut types,
        500,
        &[OCCURRENCE_KEY, CHILD],
        &[],
    );
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    complete_scope.captured_positions = crate::test_support::with_decode_context(|ctx| {
        decode(ctx, &bytes, &records, &complete_scope, &types).unwrap()
    });
    let mut roots = vec![feature(&complete_scope)];
    let root = occurrence.clone();
    let mut child = occurrence;
    let mut child_carrier = carrier.clone();
    child_carrier.id = "f3d:Design/BulkStream.dat:occurrence#child".into();
    child_carrier.occurrence_guid = CHILD.to_owned().try_into().unwrap();
    child.native_ref = Some(child_carrier.id.clone());
    child.id = cadmpeg_ir::ids::OccurrenceId::mint("synthetic:test:occurrence#child").unwrap();
    child.parent = cadmpeg_ir::products::OccurrenceParent::Occurrence {
        occurrence: root.id.clone(),
    };
    for ambiguous_full in [false, true] {
        let mut other = if ambiguous_full {
            child.clone()
        } else {
            root.clone()
        };
        other.id = cadmpeg_ir::ids::OccurrenceId::mint("synthetic:test:occurrence#other").unwrap();
        let paths = LocalOccurrencePaths::new(
            &[carrier.clone(), child_carrier.clone()],
            &[],
            &[root.clone(), child.clone(), other],
            &[],
        );
        let keys = vec![OCCURRENCE_KEY.to_owned(), CHILD.to_owned()];
        assert_eq!(
            paths.ambiguous("f3d:Design/BulkStream.dat", &keys[..1]),
            !ambiguous_full
        );
        let losses = bind_occurrences(&mut roots, std::slice::from_ref(&complete_scope), &paths);
        assert_eq!(losses.len(), usize::from(ambiguous_full));
        let FeatureDefinition::Operation(FeatureOperation::CapturedComponentPositions {
            positions,
        }) = roots[0].evaluation.definition()
        else {
            panic!("captured positions")
        };
        assert_eq!(
            positions[0].occurrence,
            (!ambiguous_full).then(|| child.id.clone())
        );
    }
}

#[test]
fn position_keeps_mixed_affine_and_native_rows_without_changing_coefficients() {
    for value in [2.0, f64::MAX] {
        let mut matrix = Transform::identity().rows();
        if value == 2.0 {
            matrix[3][3] = value;
        } else {
            matrix[0][3] = value;
        }
        let mut valid = Transform::identity().rows();
        valid[0][3] = 2.0;
        let scope = scope(&[Some(valid), Some(matrix), None]);
        let before = scope.captured_positions.clone();
        let mut losses = Vec::new();
        let FeatureDefinition::Operation(FeatureOperation::CapturedComponentPositions {
            positions,
        }) = project(&scope, &mut losses).unwrap()
        else {
            panic!("captured positions")
        };
        assert_eq!(positions.len(), 3);
        assert_eq!(
            positions
                .iter()
                .map(|position| position.selector.as_str())
                .collect::<Vec<_>>(),
            [
                "f3d:Design/BulkStream.dat:design-record#700",
                "f3d:Design/BulkStream.dat:design-record#703",
                "f3d:Design/BulkStream.dat:design-record#706",
            ]
        );
        assert_eq!(
            positions[0].transform,
            CapturedPositionTransform::Affine {
                transform: Transform::affine([
                    [1.0, 0.0, 0.0, 20.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                ])
                .unwrap()
            }
        );
        assert_eq!(
            positions[1].transform,
            CapturedPositionTransform::Native {
                scope: cadmpeg_ir::ids::NativeId::mint(scope.id.clone()).unwrap(),
                row: 1,
            }
        );
        assert_eq!(
            positions[2].transform,
            CapturedPositionTransform::Affine {
                transform: Transform::identity()
            }
        );
        assert_eq!(scope.captured_positions, before);
        assert_eq!(
            losses[0].code.to_string(),
            "f3d/feature.position-transform-unprojected"
        );
        assert!(losses[0].message.contains("row 1"));
        assert_eq!(
            before.unwrap().rows[1]
                .matrix
                .as_ref()
                .unwrap()
                .value
                .map(cadmpeg_ir::scalar::FiniteReal::get),
            matrix.into_iter().flatten().collect::<Vec<_>>().as_slice()
        );
    }
}
