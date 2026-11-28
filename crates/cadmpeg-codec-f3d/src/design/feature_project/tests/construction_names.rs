// SPDX-License-Identifier: Apache-2.0
//! A construction Feature without a stored name takes the name composed from
//! its kind spelling and feature ordinal, and the projection charges the
//! composed construction name.

use crate::loss::F3dLossCode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

/// The projected name of `scope`'s feature and the number of composed
/// construction-name notes the projection charges.
fn project(scope: DesignParameterScope) -> (Option<String>, usize) {
    let id = scope.id.clone();
    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        projection_losses,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[scope];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let name = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(id.as_str()))
        .expect("the scope's feature")
        .name
        .clone();
    let composed = projection_losses
        .iter()
        .filter(|note| {
            note.code == F3dLossCode::FeatureConstructionNameComposed.kind()
                && note.message.contains(&id)
        })
        .count();
    (name, composed)
}

fn scope(kind: DesignFeatureKind) -> DesignParameterScope {
    DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:design-parameter-scope#12",
        kind,
        12,
    )
}

#[test]
fn an_unnamed_construction_feature_charges_its_composed_name() {
    for (kind, expected) in [
        (DesignFeatureKind::WorkPlane, "WorkPlane1"),
        (DesignFeatureKind::WorkAxis, "WorkAxis1"),
        (DesignFeatureKind::WorkPoint, "WorkPoint1"),
    ] {
        assert_eq!(project(scope(kind)), (Some(expected.to_owned()), 1), "{expected}");
    }
    let mut localized = scope(DesignFeatureKind::WorkPlane);
    localized.source_kind = Some("Plan de construction".to_owned());
    assert_eq!(
        project(localized),
        (Some("Plan de construction1".to_owned()), 1)
    );
}

#[test]
fn a_stored_construction_name_or_another_family_charges_no_composed_name() {
    let mut named = scope(DesignFeatureKind::WorkPlane);
    named.display_name = Some("Mid Plane".to_owned());
    assert_eq!(project(named), (Some("Mid Plane".to_owned()), 0));
    let mut labelled = scope(DesignFeatureKind::WorkAxis);
    labelled.label = Some(crate::records::identity::Located {
        value: "Pivot".to_owned(),
        offset: 0,
    });
    assert_eq!(project(labelled), (Some("Pivot".to_owned()), 0));
    assert_eq!(
        project(scope(DesignFeatureKind::Extrude)),
        (Some("Extrude1".to_owned()), 0)
    );
}
