// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use crate::features::FinitePoint3;
use crate::ids::{AttributeId, PointId};
use crate::math::Point3;
use crate::topology::Point;

fn model() -> Model {
    let mut model = Model::default();
    model.points.push(Point::new(
        PointId::mint("test:model:point#first").unwrap(),
        FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
        None,
    ));
    model.attributes.push(SourceAttribute {
        id: AttributeId::mint("test:model:attribute#first").unwrap(),
        native_ref: Some("test:native:property#first".to_owned()),
        target: AttributeTarget::Document,
        name: "name".to_owned(),
        values: vec![AttributeValue::String("value".to_owned())],
    });
    model
}

fn agrees(expected: &Model, current: &Model) -> bool {
    let ctx = cadmpeg_test_support::service_decode_context();
    let identities = ModelRecordIndex::new(&ctx, expected)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    ModelRecordIndex::new(&ctx, current)
        .unwrap()
        .agrees_with_projection(&ctx, expected, &identities)
        .unwrap()
}

#[test]
fn full_typed_values_and_links_in_each_populated_arena_must_match() {
    let expected = model();
    assert!(agrees(&expected, &expected));
    let mut current = expected.clone();
    current.points[0].set_position(FinitePoint3::new(Point3::new(1.0, 2.0, 4.0)).unwrap());
    assert!(!agrees(&expected, &current));
    let mut current = expected.clone();
    current.attributes[0].values = vec![AttributeValue::String("changed".to_owned())];
    assert!(!agrees(&expected, &current));
    let mut current = expected.clone();
    current.attributes[0].native_ref = None;
    assert!(!agrees(&expected, &current));
    let mut current = expected.clone();
    current.attributes[0].target = AttributeTarget::Point(expected.points[0].id.clone());
    assert!(!agrees(&expected, &current));
}

#[test]
fn captured_identity_without_native_link_cannot_disappear() {
    let expected = model();
    let mut current = expected.clone();
    current.points.clear();
    assert!(!agrees(&expected, &current));
}

#[test]
fn actual_ids_select_records_without_order_or_value_matching() {
    let mut expected = model();
    let mut second = expected.points[0].clone();
    second.id = PointId::mint("test:model:point#second").unwrap();
    expected.points.push(second);
    let mut current = expected.clone();
    current.points.reverse();
    assert!(agrees(&expected, &current));
    current.points[0].id = PointId::mint("test:model:point#different").unwrap();
    assert!(!agrees(&expected, &current));
}

#[test]
fn duplicate_identity_on_either_side_cannot_prove_equality() {
    let expected = model();
    let mut duplicate = expected.clone();
    duplicate.points.push(duplicate.points[0].clone());
    assert!(!agrees(&expected, &duplicate));
    assert!(!agrees(&duplicate, &expected));
    assert!(!agrees(&duplicate, &duplicate));
}

#[test]
fn captured_population_remains_exact_after_later_enrichment() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let expected = model();
    let identities = ModelRecordIndex::new(&ctx, &expected)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    assert!(identities.contains("points", "test:model:point#first"));
    assert!(!identities.contains("attributes", "test:model:point#first"));
    let mut enriched = expected.clone();
    let mut peer = enriched.attributes[0].clone();
    peer.id = AttributeId::mint("test:model:attribute#peer").unwrap();
    enriched.attributes.push(peer);
    assert!(ModelRecordIndex::new(&ctx, &expected)
        .unwrap()
        .agrees_with_projection(&ctx, &enriched, &identities)
        .unwrap());
    enriched.points.clear();
    assert!(!ModelRecordIndex::new(&ctx, &expected)
        .unwrap()
        .agrees_with_projection(&ctx, &enriched, &identities)
        .unwrap());
}

#[test]
fn prior_working_allocation_refusal_blocks_even_empty_index_and_capture() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let empty = Model::default();
    let index = ModelRecordIndex::new(&ctx, &empty).unwrap();
    let mut storage = Vec::<u64>::new();
    assert!(ctx
        .reserve_working(&mut storage, usize::MAX, "test allocation overflow")
        .is_err());
    assert!(ModelRecordIndex::new(&ctx, &empty).is_err());
    assert!(index.owned_identities(&ctx).is_err());
    let identities = RecordIdentities {
        records: Vec::new(),
        distinct: true,
    };
    assert!(index
        .agrees_with_projection(&ctx, &empty, &identities)
        .is_err());
}

#[test]
fn reused_complete_indexes_compare_each_actual_captured_population() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = model();
    let mut first = source.clone();
    first.attributes.clear();
    let first = ModelRecordIndex::new(&ctx, &first)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    let mut second = source.clone();
    second.points.clear();
    let second = ModelRecordIndex::new(&ctx, &second)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    let mut current = source.clone();
    current.attributes[0].name = "changed".to_owned();
    let source_index = ModelRecordIndex::new(&ctx, &source).unwrap();
    let current_index = ModelRecordIndex::new(&ctx, &current).unwrap();
    assert!(current_index
        .agrees_with_index(&ctx, &source_index, &first)
        .unwrap());
    assert!(!current_index
        .agrees_with_index(&ctx, &source_index, &second)
        .unwrap());
    let missing = Model::default();
    let missing = ModelRecordIndex::new(&ctx, &missing).unwrap();
    assert!(!current_index
        .agrees_with_index(&ctx, &missing, &first)
        .unwrap());
    let mut storage = Vec::<u64>::new();
    assert!(ctx
        .reserve_working(&mut storage, usize::MAX, "test reused projection refusal")
        .is_err());
    assert!(current_index
        .agrees_with_index(&ctx, &source_index, &first)
        .is_err());
}

#[test]
fn captured_feature_compares_its_actual_serialized_regeneration_parent_only() {
    use crate::features::{
        DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
        FeatureOperation,
    };
    fn feature(key: &str, ordinal: u64) -> Feature {
        Feature {
            id: FeatureId::mint(format!("test:model:feature#{key}")).unwrap(),
            ordinal,
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            needs_recompute: false,
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
            native_ref: None,
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut source = Model::default();
    source.features = vec![
        feature("first-parent", 0),
        feature("second-parent", 1),
        feature("captured-child", 2),
        feature("peer-child", 3),
    ];
    let first_parent = source.features[0].id.clone();
    let second_parent = source.features[1].id.clone();
    let child = source.features[2].id.clone();
    let peer = source.features[3].id.clone();
    source
        .set_feature_regeneration_parent(&child, &first_parent)
        .unwrap();
    source
        .set_feature_regeneration_parent(&peer, &first_parent)
        .unwrap();
    let mut population = Model::default();
    population.features.push(source.features[2].clone());
    let members = ModelRecordIndex::new(&ctx, &population)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    let mut current = source.clone();
    current
        .set_feature_regeneration_parent(&child, &second_parent)
        .unwrap();
    assert_eq!(source.features, current.features);
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_projection(&ctx, &source, &members)
        .unwrap());
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(
            &ctx,
            &ModelRecordIndex::new(&ctx, &source).unwrap(),
            &members
        )
        .unwrap());
    current
        .set_feature_regeneration_parent(&child, &first_parent)
        .unwrap();
    current
        .set_feature_regeneration_parent(&peer, &second_parent)
        .unwrap();
    assert!(ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(
            &ctx,
            &ModelRecordIndex::new(&ctx, &source).unwrap(),
            &members
        )
        .unwrap());
}

#[test]
fn procedural_record_comparison_includes_actual_unique_carrier_wire_links() {
    use crate::geometry::{
        Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    let ctx = cadmpeg_test_support::service_decode_context();
    let curve = ProceduralCurve::new(
        ProceduralCurveId::mint("test:model:curve-construction#actual").unwrap(),
        ProceduralCurveDefinition::Exact { cache: None },
    );
    let surface = ProceduralSurface::new(
        ProceduralSurfaceId::mint("test:model:surface-construction#actual").unwrap(),
        ProceduralSurfaceDefinition::Unknown {
            record: None,
            cache: None,
        },
        None,
    );
    let mut source = Model::default();
    source.curves.push(Curve {
        id: CurveId::mint("test:model:curve#first").unwrap(),
        native_ref: None,
        source_object: None,
        parameter_range: None,
        geometry: CurveGeometry::Procedural {
            construction: curve.id.clone(),
            cache: None,
        },
    });
    source.surfaces.push(Surface {
        id: SurfaceId::mint("test:model:surface#first").unwrap(),
        native_ref: None,
        source_object: None,
        geometry: SurfaceGeometry::Procedural {
            construction: surface.id.clone(),
            cache: None,
        },
    });
    source.procedural_curves.push(curve);
    source.procedural_surfaces.push(surface);
    let mut curve_population = Model::default();
    curve_population.procedural_curves = source.procedural_curves.clone();
    let curve_members = ModelRecordIndex::new(&ctx, &curve_population)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    let mut surface_population = Model::default();
    surface_population.procedural_surfaces = source.procedural_surfaces.clone();
    let surface_members = ModelRecordIndex::new(&ctx, &surface_population)
        .unwrap()
        .owned_identities(&ctx)
        .unwrap();
    let mut current = source.clone();
    current.curves[0].id = CurveId::mint("test:model:curve#second").unwrap();
    assert_eq!(current.procedural_curves, source.procedural_curves);
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_projection(&ctx, &source, &curve_members)
        .unwrap());
    current = source.clone();
    current.surfaces[0].id = SurfaceId::mint("test:model:surface#second").unwrap();
    assert_eq!(current.procedural_surfaces, source.procedural_surfaces);
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(
            &ctx,
            &ModelRecordIndex::new(&ctx, &source).unwrap(),
            &surface_members
        )
        .unwrap());
    assert!(ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(
            &ctx,
            &ModelRecordIndex::new(&ctx, &source).unwrap(),
            &curve_members
        )
        .unwrap());
    current = source.clone();
    current.curves.clear();
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_projection(&ctx, &source, &curve_members)
        .unwrap());
    current = source.clone();
    let mut alias = current.curves[0].clone();
    alias.id = CurveId::mint("test:model:curve#alias").unwrap();
    current.curves.push(alias);
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_projection(&ctx, &source, &curve_members)
        .unwrap());
    current.curves.clear();
    let mut missing_source = source.clone();
    missing_source.curves.clear();
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_projection(&ctx, &missing_source, &curve_members)
        .unwrap());
}

#[test]
fn actual_native_reference_groups_cover_registered_arenas_and_compare_complete_values() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut source = model();
    source.points[0].native_ref = Some("test:native:entry#actual".to_owned());
    source.attributes[0].native_ref = Some("test:native:entry#actual".to_owned());
    let mut peer = source.points[0].clone();
    peer.id = PointId::mint("test:model:point#peer").unwrap();
    peer.native_ref = Some("test:native:entry#peer".to_owned());
    source.points.push(peer);
    let source_index = ModelRecordIndex::new(&ctx, &source).unwrap();
    let groups = source_index.native_reference_groups(&ctx).unwrap();
    let members = groups.get("test:native:entry#actual").unwrap();
    assert!(members.contains("points", "test:model:point#first"));
    assert!(members.contains("attributes", "test:model:attribute#first"));
    assert_eq!(members.iter().count(), 2);
    assert!(groups.get("test:native:entry#absent").is_none());
    let mut current = source.clone();
    current.points[1].set_position(FinitePoint3::new(Point3::new(9.0, 8.0, 7.0)).unwrap());
    assert!(ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(&ctx, &source_index, members)
        .unwrap());
    current.attributes[0].values = vec![AttributeValue::String("changed".to_owned())];
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(&ctx, &source_index, members)
        .unwrap());
    current = source.clone();
    current.points[0].native_ref = None;
    assert!(!ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .agrees_with_index(&ctx, &source_index, members)
        .unwrap());
    assert!(ModelRecordIndex::new(&ctx, &current)
        .unwrap()
        .contains_any(&ctx, members)
        .unwrap());
    let empty = Model::default();
    assert!(!ModelRecordIndex::new(&ctx, &empty)
        .unwrap()
        .contains_any(&ctx, members)
        .unwrap());
}

#[test]
fn native_reference_groups_observe_prior_fuse_on_an_empty_model() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let model = Model::default();
    let index = ModelRecordIndex::new(&ctx, &model).unwrap();
    assert!(index
        .native_reference_groups(&ctx)
        .unwrap()
        .get("test:native:entry#absent")
        .is_none());
    let mut storage = Vec::<u64>::new();
    assert!(ctx
        .reserve_working(&mut storage, usize::MAX, "native groups prior refusal")
        .is_err());
    assert!(index.native_reference_groups(&ctx).is_err());
}

#[test]
fn captured_identity_getter_returns_actual_arena_qualified_bytes() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = model();
    let index = ModelRecordIndex::new(&ctx, &source).unwrap();
    let members = index.owned_identities(&ctx).unwrap();
    let bytes = members.get("points", source.points[0].id.as_str()).unwrap();
    assert_eq!(bytes, source.points[0].id.as_str().as_bytes());
    let actual = members
        .iter()
        .find(|(arena, id)| *arena == "points" && *id == bytes)
        .unwrap()
        .1;
    assert!(std::ptr::eq(bytes.as_ptr(), actual.as_ptr()));
    assert!(members
        .get("attributes", source.points[0].id.as_str())
        .is_none());
    assert!(members.get("points", "test:model:point#missing").is_none());
    assert!(members
        .get("points", "test:model:point#first-suffix")
        .is_none());
    assert!(members.contains("points", source.points[0].id.as_str()));
}
