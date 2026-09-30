//! Unresolved fillet group preservation.

use crate::resolved_features::projections::sole_unresolved_fillet_group;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn a_sole_unresolved_fillet_group_carries_its_edges() {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        EdgeSelection,
    };

    let group = |edges: EdgeSelection, radius: RadiusSpec| FilletGroup {
        edges,
        radius,
        tangency_weight: None,
    };
    let native = EdgeSelection::Native("native:fillet-edges".into());
    let fillet = |groups: Vec<FilletGroup>| {
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: groups
                .try_into()
                .expect("a fillet keeps one or more groups"),
        })
    };

    let definition = fillet(vec![group(
        native.clone(),
        RadiusSpec::Unresolved {
            form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
        },
    )]);
    let carried = sole_unresolved_fillet_group(&definition)
        .expect("a sole group without a radius is carried out of the check");
    assert_eq!(carried.0, &native);

    assert_eq!(
        sole_unresolved_fillet_group(&fillet(vec![group(
            native.clone(),
            RadiusSpec::Constant {
                radius: cadmpeg_ir::scalar::PositiveLength::new(2.0).expect("a positive radius"),
            },
        )])),
        None
    );
    assert_eq!(
        sole_unresolved_fillet_group(&fillet(vec![
            group(
                native.clone(),
                RadiusSpec::Unresolved {
                    form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable)
                }
            ),
            group(
                EdgeSelection::Unresolved,
                RadiusSpec::Unresolved {
                    form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable)
                }
            ),
        ])),
        None
    );
    assert_eq!(
        sole_unresolved_fillet_group(&FeatureDefinition::Operation(FeatureOperation::Chamfer {
            groups: vec![cadmpeg_ir::features::edge_treatments::ChamferGroup {
                edges: native,
                spec: cadmpeg_ir::features::edge_treatments::ChamferSpec::Unresolved {
                    form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::Distance)
                },
            }]
            .try_into()
            .expect("a chamfer keeps one or more groups"),
            flip_direction: false,
        })),
        None
    );
}

#[test]
fn a_sole_unresolved_fillet_group_carries_its_tangency_weight() {
    use cadmpeg_ir::features::{
        edge_treatments::{FilletGroup, RadiusSpec},
        EdgeSelection,
    };

    let weight = cadmpeg_ir::scalar::FiniteReal::new(0.75).expect("a finite tangency weight");
    let fillet = |tangency_weight| {
        FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: vec![FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Unresolved {
                    form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                },
                tangency_weight,
            }]
            .try_into()
            .expect("a fillet keeps one or more groups"),
        })
    };

    assert_eq!(
        sole_unresolved_fillet_group(&fillet(Some(weight)))
            .expect("a sole group without a radius is carried out of the check")
            .1,
        Some(weight)
    );
    assert_eq!(
        sole_unresolved_fillet_group(&fillet(None))
            .expect("a sole group without a radius is carried out of the check")
            .1,
        None
    );
}
