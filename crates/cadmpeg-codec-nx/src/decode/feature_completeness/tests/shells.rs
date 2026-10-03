// SPDX-License-Identifier: Apache-2.0

use crate::decode::feature_completeness::shell_definition_is_incomplete;

#[test]
fn nx_shell_completeness_requires_each_construction_field() {
    use cadmpeg_ir::features::{
        BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, ShellJoin, ShellMode,
    };
    use cadmpeg_ir::ids::{BodyId, FaceId};

    let incomplete = FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: FaceSelection::Unresolved,
        thickness: None,
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    });
    assert!(shell_definition_is_incomplete(&incomplete));

    let complete = FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: Some(BodySelection::Bodies(
            cadmpeg_ir::features::DistinctMembers::try_from(
                vec![BodyId::mint("test:model:body#shell").expect("identity grammar")],
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("distinct bodies"),
        )),
        removed_faces: FaceSelection::Faces(vec![
            FaceId::mint("test:model:face#opening").expect("identity grammar")
        ]),
        thickness: Some(cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap()),
        outward: Some(false),
        mode: Some(ShellMode::Skin),
        join: Some(ShellJoin::Intersection),
        resolve_intersections: Some(true),
        allow_self_intersections: Some(false),
    });
    assert!(!shell_definition_is_incomplete(&complete));
    assert_eq!(complete.body_output_family(), Some("shell"));
}
