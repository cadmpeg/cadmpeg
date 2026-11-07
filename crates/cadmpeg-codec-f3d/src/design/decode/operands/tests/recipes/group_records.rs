// SPDX-License-Identifier: Apache-2.0

use crate::records::decal::DesignRecordHeader;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::test_support::indexed_header;

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn group(scope_record_index: u32, record_index: u32) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:group#{scope_record_index}-{record_index}"),
        "scope_record_index": scope_record_index,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "class_tag": "332",
        "members": [],
        "frame": {
            "opaque_index": 1,
            "opaque_scalar": 0.0,
            "variant": false
        },
        "role": DesignOperandRole::ROLE_0X10.raw(),
        "paired_class_tag": "259",
    }))
    .unwrap()
}

#[test]
fn a_scope_group_record_is_not_a_face_operand() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"306", 100);
    indexed_header(&mut bytes, *b"259", 100);
    indexed_header(&mut bytes, *b"408", 101);
    indexed_header(&mut bytes, *b"414", 102);
    indexed_header(&mut bytes, *b"423", 103);
    let recipe_at = bytes.len() + 4;
    bytes.extend_from_slice(&24u32.to_le_bytes());
    bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"306", 104);
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Shell,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![100]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let header = DesignRecordHeader {
        id: format!("{STREAM}:design-record-header#100"),
        record_index: 100,
        class_tag: "306".to_owned().try_into().unwrap(),
        byte_offset: Some(0),
    };
    let recipe = ConstructionRecipe {
        id: format!("{STREAM}:construction-recipe#{recipe_at}"),
        byte_offset: Some(u64::try_from(recipe_at).unwrap()),
        kind: ConstructionRecipeKind::BoundedFace,
        design: None,
        recipe_index: 0,
        record_index: None,
    };
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 104],
        &[100],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let operands = |groups: &[DesignConstructionOperandGroup]| {
                let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                    cadmpeg_ir::CodecFormat::F3d,
                    ctx,
                );
                crate::design::decode::operands::decode_face_operands(
                    ctx,
                    &mut parts,
                    scan,
                    std::slice::from_ref(&scope),
                    groups,
                    std::slice::from_ref(&header),
                    std::slice::from_ref(&recipe),
                )
                .unwrap()
                .iter()
                .map(crate::records::topology::face::DesignFaceOperand::record_index)
                .collect::<Vec<_>>()
            };
            assert_eq!(operands(&[]), [100]);
            assert_eq!(operands(&[group(51, 100)]), [100]);
            assert_eq!(operands(&[group(50, 200)]), [100]);
            assert!(operands(&[group(50, 100)]).is_empty());
        });
    });
}

#[test]
fn malformed_face_recipe_is_given_up_and_sibling_survives() {
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    for (base, valid) in [(100u32, false), (200, true)] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"306", base);
        indexed_header(&mut bytes, *b"259", base);
        indexed_header(&mut bytes, *b"408", base + 1);
        indexed_header(&mut bytes, *b"414", base + 2);
        indexed_header(&mut bytes, *b"423", base + 3);
        let recipe_at = bytes.len() + 4;
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"bounded_face_recipe_data");
        if valid {
            for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"306", base + 4);
        headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{base}"),
            record_index: base,
            class_tag: "306".to_owned().try_into().unwrap(),
            byte_offset: Some(u64::try_from(start).unwrap()),
        });
        recipes.push(ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{recipe_at}"),
            byte_offset: Some(u64::try_from(recipe_at).unwrap()),
            kind: ConstructionRecipeKind::BoundedFace,
            design: None,
            recipe_index: 0,
            record_index: None,
        });
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Shell,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 104, 200, 201, 202, 203, 204],
        &[100, 200],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            let operands = crate::design::decode::operands::decode_face_operands(
                ctx,
                &mut parts,
                scan,
                &[scope],
                &[],
                &headers,
                &recipes,
            )
            .unwrap();
            assert_eq!(
                operands
                    .iter()
                    .map(crate::records::topology::face::DesignFaceOperand::record_index)
                    .collect::<Vec<_>>(),
                [200]
            );
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
}

#[test]
fn malformed_edge_recipe_is_given_up_and_sibling_survives() {
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    for (base, valid) in [(100u32, false), (200, true)] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"306", base);
        indexed_header(&mut bytes, *b"259", base);
        indexed_header(&mut bytes, *b"408", base + 1);
        indexed_header(&mut bytes, *b"414", base + 2);
        indexed_header(&mut bytes, *b"423", base + 3);
        indexed_header(&mut bytes, *b"122", 0);
        let recipe_at = bytes.len() + 4;
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"edge_recipe_data");
        if valid {
            for value in [-1i32, -1, 2, 0, -1, 1, -1, 7] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"306", base + 4);
        headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{base}"),
            record_index: base,
            class_tag: "306".to_owned().try_into().unwrap(),
            byte_offset: Some(u64::try_from(start).unwrap()),
        });
        recipes.push(ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{recipe_at}"),
            byte_offset: Some(u64::try_from(recipe_at).unwrap()),
            kind: ConstructionRecipeKind::Edge,
            design: None,
            recipe_index: 7,
            record_index: None,
        });
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Fillet,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:group#50"), "scope_record_index": 50,
        "scope_reference_ordinal": 0, "record_index": 50, "class_tag": "332",
        "members": [100, 200], "role": DesignOperandRole::ROLE_0X10.raw(),
        "paired_class_tag": "259",
        "frame": {"opaque_index": 1, "opaque_scalar": 0.0, "variant": false}
    }))
    .unwrap();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 104, 200, 201, 202, 203, 204],
        &[100, 200],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            let operands = crate::design::decode::operands::decode_edge_operands(
                ctx,
                &mut parts,
                scan,
                &[scope],
                &[group],
                &headers,
                &recipes,
            )
            .unwrap();
            assert_eq!(
                operands
                    .iter()
                    .map(crate::records::topology::edge_identity::DesignEdgeOperand::record_index)
                    .collect::<Vec<_>>(),
                [200]
            );
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
}

#[test]
fn malformed_vertex_recipe_is_given_up_and_sibling_survives() {
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    for (base, valid) in [(100u32, false), (200, true)] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"369", base);
        indexed_header(&mut bytes, *b"261", base);
        indexed_header(&mut bytes, *b"408", base + 1);
        indexed_header(&mut bytes, *b"414", base + 2);
        indexed_header(&mut bytes, *b"423", base + 3);
        let recipe_at = bytes.len() + 4;
        bytes.extend_from_slice(&18u32.to_le_bytes());
        bytes.extend_from_slice(b"vertex_recipe_data");
        if valid {
            for value in [-1i32, 3, 1, -1, 2, -1, 0, -1, 0, 0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"370", base + 5);
        headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{base}"),
            record_index: base,
            class_tag: "369".to_owned().try_into().unwrap(),
            byte_offset: Some(u64::try_from(start).unwrap()),
        });
        recipes.push(ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{recipe_at}"),
            byte_offset: Some(u64::try_from(recipe_at).unwrap()),
            kind: ConstructionRecipeKind::Vertex,
            design: None,
            recipe_index: 9,
            record_index: None,
        });
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::Fillet,
        50,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![100, 200]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:group#50"), "scope_record_index": 50,
        "scope_reference_ordinal": 0, "record_index": 50, "class_tag": "332",
        "members": [100, 200], "role": DesignOperandRole::ROLE_0X10.raw(),
        "paired_class_tag": "259",
        "frame": {"opaque_index": 1, "opaque_scalar": 0.0, "variant": false}
    }))
    .unwrap();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 105, 200, 201, 202, 203, 205],
        &[100, 200],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            let operands = crate::design::decode::operands::decode_edge_treatment_vertex_operands(
                ctx,
                &mut parts,
                scan,
                &[scope],
                &[group],
                &headers,
                &recipes,
            )
            .unwrap();
            assert_eq!(
                operands
                    .iter()
                    .map(|operand| operand.recipe.record_index())
                    .collect::<Vec<_>>(),
                [200]
            );
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
}

#[test]
fn work_point_gives_up_broken_vertex_and_keeps_sibling() {
    use crate::records::feature::scope::DesignScopePayloadMut;
    use crate::records::feature::work_geometry::{
        DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier,
        DesignWorkPointRule,
    };
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    for (base, valid) in [(100u32, false), (200, true)] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"369", base);
        indexed_header(&mut bytes, *b"261", base);
        indexed_header(&mut bytes, *b"408", base + 1);
        indexed_header(&mut bytes, *b"414", base + 2);
        indexed_header(&mut bytes, *b"423", base + 3);
        let recipe_at = bytes.len() + 4;
        bytes.extend_from_slice(&18u32.to_le_bytes());
        bytes.extend_from_slice(b"vertex_recipe_data");
        if valid {
            for value in [-1i32, 3, 1, -1, 2, -1, 0, -1, 0, 0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"370", base + 5);
        headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{base}"),
            record_index: base,
            class_tag: "369".to_owned().try_into().unwrap(),
            byte_offset: Some(u64::try_from(start).unwrap()),
        });
        recipes.push(ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{recipe_at}"),
            byte_offset: Some(u64::try_from(recipe_at).unwrap()),
            kind: ConstructionRecipeKind::Vertex,
            design: None,
            recipe_index: 9,
            record_index: None,
        });
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#50"),
        DesignFeatureKind::WorkPoint,
        50,
    );
    let inputs = [100, 200]
        .into_iter()
        .map(|index| DesignWorkPointInput::try_new(index, None, None).unwrap())
        .collect();
    let rule = DesignWorkPointRule::from_serialized(99, inputs).unwrap();
    let DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() else {
        panic!("work point fixture")
    };
    *slot = Some(DesignWorkPointConstruction {
        point_record_index: 50,
        point_record_byte_offset: None,
        position: crate::test_support::reals([0.0, 0.0, 0.0]),
        position_offset: None,
        rule,
        reference_type_offset: None,
    });
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(
        &bytes,
        &[100, 101, 102, 103, 105, 200, 201, 202, 203, 205],
        &[100, 200],
    );
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            crate::design::decode::operands::bind_work_point_input_carriers(
                (ctx, &mut parts),
                scan,
                std::slice::from_mut(&mut scope),
                &headers,
                &recipes,
                &[],
                &[],
            )
            .unwrap();
            let inputs = scope.work_point_construction().unwrap().rule.inputs();
            assert!(inputs[0].carrier().is_none());
            assert!(matches!(
                inputs[1].carrier(),
                Some(DesignWorkPointInputCarrier::VertexRecipe { .. })
            ));
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
}

#[test]
fn work_plane_gives_up_broken_vertex_and_keeps_sibling_plane() {
    use crate::records::parameters::{
        DesignParameter, DesignParameterContent, DesignParameterOwner, DesignParameterOwnerContent,
        DesignParameterSource,
    };
    let mut bytes = Vec::new();
    let mut headers = Vec::new();
    let mut recipes = Vec::new();
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    for base in [100u32, 200, 300, 400, 500, 600] {
        let start = bytes.len();
        indexed_header(&mut bytes, *b"369", base);
        indexed_header(&mut bytes, *b"261", base);
        indexed_header(&mut bytes, *b"408", base + 1);
        indexed_header(&mut bytes, *b"414", base + 2);
        indexed_header(&mut bytes, *b"423", base + 3);
        let recipe_at = bytes.len() + 4;
        bytes.extend_from_slice(&18u32.to_le_bytes());
        bytes.extend_from_slice(b"vertex_recipe_data");
        if base != 100 {
            for value in [-1i32, 3, 1, -1, 2, -1, 0, -1, 0, 0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        indexed_header(&mut bytes, *b"370", base + 5);
        primary.extend([base, base + 1, base + 2, base + 3, base + 5]);
        secondary.push(base);
        headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{base}"),
            record_index: base,
            class_tag: "369".to_owned().try_into().unwrap(),
            byte_offset: Some(u64::try_from(start).unwrap()),
        });
        recipes.push(ConstructionRecipe {
            id: format!("{STREAM}:construction-recipe#{recipe_at}"),
            byte_offset: Some(u64::try_from(recipe_at).unwrap()),
            kind: ConstructionRecipeKind::Vertex,
            design: None,
            recipe_index: 9,
            record_index: None,
        });
    }
    let mut scopes = Vec::new();
    let mut owners = Vec::new();
    let mut parameters = Vec::new();
    for (scope_index, vertices, owner_index) in
        [(50, [100, 200, 300], 900), (51, [400, 500, 600], 910)]
    {
        let mut scope = DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#{scope_index}"),
            DesignFeatureKind::WorkPlane,
            scope_index,
        );
        scope
            .try_edit(|draft| {
                draft.frame_length = 256;
                draft.paired_byte_offset = 256;
                draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                    1,
                    vertices[0],
                    vertices[1],
                    vertices[2],
                    owner_index,
                ]);
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.with_work_plane_transform(
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap(),
        );
        scope.with_work_plane_reference(owner_index);
        scopes.push(scope);
        owners.push(
            DesignParameterOwner::try_from(DesignParameterOwnerContent {
                id: format!("{STREAM}:owner#{owner_index}"),
                class_tag: "365".to_owned().try_into().unwrap(),
                record_index: owner_index,
                scope_record_index: Some(scope_index),
                local_ordinal: None,
                evaluated_value: 0.0,
                parameter_record_index: owner_index + 1,
                owned_ordinal: 0,
                variant: None,
                companion_record_index: owner_index + 2,
            })
            .unwrap(),
        );
        parameters.push(
            DesignParameter::authored(DesignParameterContent {
                id: format!("{STREAM}:parameter#{}", owner_index + 1),
                class_tag: "365".to_owned().try_into().unwrap(),
                record_index: owner_index + 1,
                source_ordinal: 0,
                source: DesignParameterSource::authored(
                    "ExtraOffset".into(),
                    Some(owner_index),
                    None,
                )
                .unwrap(),
                expression: "0".into(),
                unit: None,
                name: "ExtraOffset".into(),
                evaluated_value: 0.0,
            })
            .unwrap(),
        );
    }
    let metadata =
        crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &primary, &secondary);
    let archive = crate::test_support::body_reference_tail_test::archive(&bytes, &metadata);
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let mut parts = cadmpeg_ir::source_fidelity::parts::PartOutput::new(
                cadmpeg_ir::CodecFormat::F3d,
                ctx,
            );
            crate::design::decode::operands::bind_work_plane_constructions(
                (ctx, &mut parts),
                scan,
                &mut scopes,
                &headers,
                &recipes,
                &owners,
                &parameters,
            )
            .unwrap();
            assert!(scopes[0].work_plane_construction().is_none());
            assert!(scopes[1].work_plane_construction().is_some());
            let (_, native, _, losses) = parts.into_parts();
            assert_eq!(
                native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
                1
            );
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code.local_code(), "record.unreadable");
        });
    });
}
