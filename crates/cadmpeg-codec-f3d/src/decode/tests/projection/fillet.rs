use crate::decode::feature_definition_is_incomplete;
use cadmpeg_ir::features::edge_treatments::{FilletGroup, RadiusSpec};
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation, NonEmptyMembers};

#[test]
fn fillet_with_resolved_edges_and_unresolved_radius_is_incomplete() {
    for (radius, incomplete) in [
        (RadiusSpec::Unresolved { form: None }, true),
        (
            RadiusSpec::Constant {
                radius: cadmpeg_ir::scalar::PositiveLength::new(7.0).unwrap(),
            },
            false,
        ),
    ] {
        let definition = FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: NonEmptyMembers::one(FilletGroup {
                edges: EdgeSelection::All,
                radius,
                tangency_weight: None,
            }),
        });
        assert_eq!(feature_definition_is_incomplete(&definition), incomplete);
    }
}
