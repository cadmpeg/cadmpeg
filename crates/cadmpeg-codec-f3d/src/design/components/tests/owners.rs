// SPDX-License-Identifier: Apache-2.0

use super::{
    carrier, external_owner_table, nested_fixture, two_component_stream, BTreeSet,
    DesignComponentOccurrencePlacement, COMPONENT_ROOT, DOCUMENT_ROOT,
};

#[test]
fn a_bound_definition_retains_its_identity_without_an_occurrence() {
    let components = two_component_stream(Some(276));
    let bodies = BTreeSet::new();
    let sources = super::super::LocalComponentSources {
        hidden_bodies: None,
        scopes: &[],
        occurrences: &[],
        paths: &[],
        components: &components,
        body_bindings: &[],
        bodies: Some(&bodies),
        root_ordinal_start: 0,
        limits: cadmpeg_core::decode::ResourceLimits::default(),
    };
    let projection = super::super::project_local_components(&sources).unwrap();
    let id = crate::ids::neutral_component_id(&super::guid(super::COMPONENT_CONTEXT));
    assert!(projection.occurrences.is_empty());
    assert!(projection.losses.is_empty());
    assert_eq!(projection.definitions.len(), 2);
    let definition = projection
        .definitions
        .iter()
        .find(|definition| definition.id == id)
        .unwrap();
    assert_eq!(definition.source_name.as_deref(), Some("Bracket"));
    assert!(definition.bodies.is_empty());
    assert_eq!(
        projection.component_definitions_by_record,
        std::collections::BTreeMap::from([
            ((super::STREAM.to_owned(), 159), id),
            (
                (super::STREAM.to_owned(), 276),
                crate::ids::neutral_component_id(&super::guid(super::ROOT_CONTEXT)),
            ),
        ])
    );
}

#[test]
fn a_single_implicit_root_keeps_document_scope_without_an_occurrence() {
    for root in [None, Some(999)] {
        let mut components = two_component_stream(root);
        components
            .get_mut(super::STREAM)
            .unwrap()
            .roots
            .remove(&159);
        let sources = super::super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &[],
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: None,
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let projection = super::super::project_local_components(&sources).unwrap();
        assert!(projection.definitions.is_empty());
        assert!(projection.component_definitions_by_record.is_empty());
        assert!(projection.occurrences.is_empty());
        assert!(projection.losses.is_empty());
    }
}

#[test]
fn an_unplaced_definition_retains_its_owned_body() {
    let components = two_component_stream(Some(276));
    let projection =
        super::project_carriers(&[], &components, &[super::COMPONENT_BODY, super::ROOT_BODY]);
    assert!(projection.occurrences.is_empty());
    assert_eq!(projection.definitions.len(), 2);
    for (context, body) in [
        (super::COMPONENT_CONTEXT, super::COMPONENT_BODY),
        (super::ROOT_CONTEXT, super::ROOT_BODY),
    ] {
        let id = crate::ids::neutral_component_id(&super::guid(context));
        let definition = projection
            .definitions
            .iter()
            .find(|definition| definition.id == id)
            .unwrap();
        assert_eq!(
            definition.bodies,
            [cadmpeg_ir::ids::BodyId::mint(body).unwrap()]
        );
    }
}

#[test]
fn an_identity_owner_carrier_places_external_members_when_body_projection_is_deferred() {
    let components = two_component_stream(Some(276));
    let carriers = [carrier(
        225,
        "00000000-0000-4000-8000-000000000225",
        DesignComponentOccurrencePlacement::Identity {
            ordinal: std::num::NonZeroU32::new(2).unwrap(),
        },
        DOCUMENT_ROOT,
    )];
    assert!(carriers[0].transform().is_none());
    let sources = super::super::LocalComponentSources {
        hidden_bodies: None,
        scopes: &[],
        occurrences: &carriers,
        paths: &[],
        components: &components,
        body_bindings: &[],
        bodies: None,
        root_ordinal_start: 0,
        limits: cadmpeg_core::decode::ResourceLimits::default(),
    };
    let local = super::super::project_local_components(&sources).unwrap();
    assert!(local.occurrences.is_empty());
    let projection = crate::xref::project_occurrences(
        &external_owner_table(COMPONENT_ROOT),
        &sources,
        &local.occurrences,
    )
    .unwrap();
    assert!(projection.losses.is_empty());
    assert_eq!(projection.occurrences.len(), 1);
    assert_eq!(projection.occurrences[0].transform.rows()[0][3], 20.0);
}

#[test]
fn a_unique_owner_chain_composes_and_unresolved_owners_keep_the_root_placement() {
    for case in ["unique", "ambiguous", "unreachable", "cycle"] {
        let (mut carriers, components) = nested_fixture();
        if case != "ambiguous" {
            carriers.pop();
        }
        if case == "unreachable" {
            carriers[0].owner_entity = 999;
        }
        if case == "cycle" {
            carriers[0].owner_entity = COMPONENT_ROOT;
        }
        let bodies = BTreeSet::new();
        let sources = super::super::LocalComponentSources {
            hidden_bodies: None,
            scopes: &[],
            occurrences: &carriers,
            paths: &[],
            components: &components,
            body_bindings: &[],
            bodies: Some(&bodies),
            root_ordinal_start: 0,
            limits: cadmpeg_core::decode::ResourceLimits::default(),
        };
        let projection =
            crate::xref::project_occurrences(&external_owner_table(COMPONENT_ROOT), &sources, &[])
                .unwrap();
        if case == "unique" {
            assert!(projection.losses.is_empty());
            assert_eq!(projection.occurrences.len(), 1);
            assert_eq!(
                projection.occurrences[0].transform.rows(),
                [
                    [0.0, -1.0, 0.0, 30.0],
                    [1.0, 0.0, 0.0, 60.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0]
                ]
            );
        } else {
            assert_eq!(projection.occurrences.len(), 1, "{case}");
            assert_eq!(
                projection.occurrences[0].parent,
                cadmpeg_ir::products::OccurrenceParent::Root {}
            );
            assert_eq!(projection.occurrences[0].transform.rows()[0][3], 20.0);
            assert_eq!(projection.losses.len(), 1, "{case}");
            assert_eq!(
                projection.losses[0].code,
                crate::loss::F3dLossCode::XrefOwnerUnresolved.kind()
            );
        }
    }
}
