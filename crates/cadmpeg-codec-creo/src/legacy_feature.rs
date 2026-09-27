// SPDX-License-Identifier: Apache-2.0
//! Feature-owned joins from the legacy ASCII persistence graph.

use crate::legacy::value_index;
use std::collections::{BTreeMap, BTreeSet};

use crate::curve::CurveTopologyRow;
use crate::legacy::{self, NumericPayload, ObjectPayload, ObjectRecord, Persistence};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

const ROUND_SCHEMA_CLASS: i32 = 913;
const DIMENSION_TYPE: i32 = 8;
const ROUND_DIMENSION_KIND: i32 = 3;

/// The state of a legacy round's design-radius dimension join.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LegacyRoundRadius {
    /// The complete dimension table has no radius row for this feature.
    NotPresent,
    /// All feature-owned radius rows carry the same positive value.
    Constant(PositiveReal),
    /// A matching radius row is malformed, non-positive, or disagrees with
    /// another matching row.
    Ambiguous,
}

/// A legacy feature record whose schema class identifies a round operation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LegacyRoundFeature {
    /// Feature identifier from the direct feature node's id field.
    pub(crate) feature_id: u32,
    /// Radius state joined from `Sld_FullData.dim_array`.
    pub(crate) radius: LegacyRoundRadius,
    /// Feature-owned visible curve identities when their rows are unique.
    pub(crate) edge_ids: Option<Vec<u32>>,
    /// Byte offset of the feature node.
    pub(crate) offset: usize,
}

/// Legacy feature joins needed by the neutral feature transfer.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LegacyFeatureScan {
    /// Unique legacy round feature records.
    pub(crate) rounds: Vec<LegacyRoundFeature>,
}

type ObjectIndex<'a> = BTreeMap<String, &'a ObjectRecord>;
type ChildrenIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a ObjectRecord>>;
type IntegerIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a legacy::RealRecord>>;

struct Index<'a> {
    objects: ObjectIndex<'a>,
    children: ChildrenIndex<'a>,
    integers: IntegerIndex<'a>,
    reals: RealIndex<'a>,
}

impl<'a> Index<'a> {
    fn build(ctx: &DecodeContext<'_>, persistence: &'a Persistence) -> Result<Option<Self>, CodecError> {
        let mut objects = BTreeMap::new();
        let mut children = BTreeMap::new();
        for object in &persistence.objects {
            let id = legacy::checked_object_node_id(ctx, object.offset)?;
            match objects.entry(id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    ctx.charge_collection_items(1, "creo legacy feature object index nodes")?;
                    entry.insert(object);
                }
                std::collections::btree_map::Entry::Occupied(_) => return Ok(None),
            }
            if let Some(parent) = object.parent {
                match children.entry((parent, object.name.as_str())) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        ctx.charge_collection_items(1, "creo legacy feature child index nodes")?;
                        let mut rows = Vec::new();
                        ctx.try_reserve_items(&mut rows, 1, "creo legacy feature child index rows")?;
                        rows.push(object);
                        entry.insert(rows);
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let rows = entry.get_mut();
                        ctx.try_reserve_items(rows, 1, "creo legacy feature child index rows")?;
                        rows.push(object);
                    }
                }
            }
        }
        Ok(Some(Self {
            objects,
            children,
            integers: value_index(ctx, &persistence.integer_values.rows)?,
            reals: value_index(ctx, &persistence.real_values.rows)?,
        }))
    }

    fn children(&self, parent: usize, name: &'static str) -> &[&'a ObjectRecord] {
        self.children.get(&(parent, name)).map_or(&[], Vec::as_slice)
    }

    fn unique_child(&self, parent: usize, name: &'static str) -> Option<&ObjectRecord> {
        let children = self.children(parent, name);
        let [child] = children else {
            return None;
        };
        Some(*child)
    }

    fn unique_integer_scalar(&self, parent: usize, name: &str) -> Option<i32> {
        let records = self.integers.get(&(parent, name))?;
        let [record] = records.as_slice() else {
            return None;
        };
        match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array(_) => None,
        }
    }

    fn unique_real_scalar(&self, parent: usize, name: &str) -> Option<f64> {
        let records = self.reals.get(&(parent, name))?;
        let [record] = records.as_slice() else {
            return None;
        };
        match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array(_) => None,
        }
    }
}

/// Decode feature-owned round records from one legacy persistence graph.
pub(crate) fn scan(
    ctx: &DecodeContext<'_>,
    persistence: &Persistence,
    topology_rows: &[CurveTopologyRow],
) -> Result<LegacyFeatureScan, CodecError> {
    let Some(index) = Index::build(ctx, persistence)? else {
        return Ok(LegacyFeatureScan::default());
    };
    let Some(features_root) = unique_root(&index, "Sld_Features") else {
        return Ok(LegacyFeatureScan::default());
    };
    let radius_rows = full_data_dimension_rows(ctx, &index)?;
    let mut rounds = BTreeMap::new();
    let mut ambiguous_feature_ids = BTreeSet::new();
    let feature_nodes = index
        .children(features_root.offset, "first_feat_ptr")
        .iter()
        .chain(index.children(features_root.offset, "next_feat_ptr"));
    for feature in feature_nodes {
        let Some(feature_id) = index
            .unique_integer_scalar(feature.offset, "id")
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(feature_type_object) = index.unique_child(feature.offset, "feat_type_ptr") else {
            continue;
        };
        let Some(schema_class) = index.unique_integer_scalar(feature_type_object.offset, "type")
        else {
            continue;
        };
        if schema_class != ROUND_SCHEMA_CLASS {
            continue;
        }
        let round = LegacyRoundFeature {
            feature_id,
            radius: radius_rows
                .as_deref()
                .map_or(LegacyRoundRadius::NotPresent, |rows| {
                    round_radius(rows, &index, feature_id)
                }),
            edge_ids: unique_feature_edge_ids(ctx, topology_rows, feature_id)?,
            offset: feature.offset,
        };
        match rounds.entry(feature_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo legacy round index nodes")?;
                entry.insert(round);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(round);
                if !ambiguous_feature_ids.contains(&feature_id) {
                    ctx.charge_collection_items(1, "creo legacy ambiguous round IDs")?;
                    ambiguous_feature_ids.insert(feature_id);
                }
            }
        }
    }
    let mut visible_rounds = Vec::new();
    ctx.try_reserve_items(
        &mut visible_rounds,
        rounds.len() - ambiguous_feature_ids.len(),
        "creo legacy round results",
    )?;
    visible_rounds.extend(rounds.into_iter().filter_map(|(feature_id, round)| {
        (!ambiguous_feature_ids.contains(&feature_id)).then_some(round)
    }));
    Ok(LegacyFeatureScan {
        rounds: visible_rounds,
    })
}

fn unique_root<'a>(index: &'a Index<'a>, name: &str) -> Option<&'a ObjectRecord> {
    let mut roots = index
        .objects
        .values()
        .filter(|object| object.name == name && object.parent.is_none())
        .copied();
    let root = roots.next()?;
    roots.next().is_none().then_some(root)
}

fn full_data_dimension_rows<'a>(
    ctx: &DecodeContext<'_>,
    index: &'a Index<'a>,
) -> Result<Option<Vec<&'a ObjectRecord>>, CodecError> {
    let Some(root) = unique_root(index, "Sld_FullData") else {
        return Ok(None);
    };
    let arrays = index.children(root.offset, "dim_array");
    let [array] = arrays else {
        return Ok(None);
    };
    let array = *array;
    if !array.payload.is_complete() {
        return Ok(None);
    }
    let ObjectPayload::Array { elements, .. } = &array.payload else {
        return Ok(None);
    };
    let mut seen = BTreeSet::new();
    let mut rows = Vec::new();
    ctx.try_reserve_items(&mut rows, elements.len(), "creo legacy dimension rows")?;
    for element_id in elements {
        if seen.contains(element_id.as_str()) {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo legacy dimension identities")?;
        seen.insert(element_id.as_str());
        let Some(element) = index.objects.get(element_id.as_str()).copied() else {
            return Ok(None);
        };
        if element.parent != Some(array.offset) || element.name != "dim_array" {
            return Ok(None);
        }
        rows.push(element);
    }
    Ok(Some(rows))
}

fn round_radius(rows: &[&ObjectRecord], index: &Index<'_>, feature_id: u32) -> LegacyRoundRadius {
    let Ok(feature_id) = i32::try_from(feature_id) else {
        return LegacyRoundRadius::NotPresent;
    };
    let mut found = false;
    let mut first_value: Option<PositiveReal> = None;
    for row in rows {
        let fields = (
            index.unique_integer_scalar(row.offset, "type"),
            index.unique_integer_scalar(row.offset, "dim_type"),
            index.unique_integer_scalar(row.offset, "feat_id"),
        );
        if fields
            != (
                Some(DIMENSION_TYPE),
                Some(ROUND_DIMENSION_KIND),
                Some(feature_id),
            )
        {
            continue;
        }
        found = true;
        let Some(dimension_data) = index.unique_child(row.offset, "dim_dat_ptr") else {
            return LegacyRoundRadius::Ambiguous;
        };
        let Some(value) = index.unique_real_scalar(dimension_data.offset, "value") else {
            return LegacyRoundRadius::Ambiguous;
        };
        let Some(value) = PositiveReal::new(value) else {
            return LegacyRoundRadius::Ambiguous;
        };
        match first_value {
            Some(first) if value.get().to_bits() != first.get().to_bits() => {
                return LegacyRoundRadius::Ambiguous;
            }
            None => first_value = Some(value),
            _ => {}
        }
    }
    if !found {
        return LegacyRoundRadius::NotPresent;
    }
    let Some(first) = first_value else {
        return LegacyRoundRadius::Ambiguous;
    };
    LegacyRoundRadius::Constant(first)
}

fn unique_feature_edge_ids(
    ctx: &DecodeContext<'_>,
    rows: &[CurveTopologyRow],
    feature_id: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows.iter().filter(|row| row.feature_id == feature_id) {
        if seen.contains(&row.id) {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo legacy round edge identities")?;
        seen.insert(row.id);
        ctx.try_reserve_items(&mut ids, 1, "creo legacy round edge IDs")?;
        ids.push(row.id);
    }
    Ok((!ids.is_empty()).then_some(ids))
}

#[cfg(test)]
mod tests {
    use super::{scan as scan_checked, LegacyRoundRadius};
    use crate::curve::CurveTopologyRow;
    use crate::legacy::{
        IntegerPayload, ObjectPayload, Persistence, Real, RealPayload, ValueRecord,
    };
    use crate::test_support::{fixture_offset, object};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    fn scan(
        persistence: &Persistence,
        topology_rows: &[crate::curve::CurveTopologyRow],
    ) -> super::LegacyFeatureScan {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
        scan_checked(&ctx, persistence, topology_rows).expect("service admits legacy features")
    }

    fn scan_with_collection_limit(
        persistence: &Persistence,
        topology_rows: &[CurveTopologyRow],
        limit: u64,
    ) -> Result<super::LegacyFeatureScan, CodecError> {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty test root");
        scan_checked(&ctx, persistence, topology_rows)
    }

    fn assert_collection_refusal(
        persistence: &Persistence,
        topology_rows: &[CurveTopologyRow],
        operation: &'static str,
    ) {
        let refusal = (0..512).find_map(|limit| {
            let Err(CodecError::ResourceLimit(refusal)) =
                scan_with_collection_limit(persistence, topology_rows, limit)
            else {
                return None;
            };
            (refusal.operation == operation).then_some(refusal)
        });
        assert!(matches!(
            refusal,
            Some(limit) if limit.dimension == ResourceDimension::CollectionItems
        ), "missing collection refusal for {operation}");
    }

    fn integer(
        parent: &str,
        name: &str,
        value: i32,
        offset: usize,
    ) -> crate::legacy::IntegerRecord {
        ValueRecord {
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: IntegerPayload::Scalar { value },
            offset,
        }
    }

    fn real(parent: &str, value: f64, offset: usize) -> crate::legacy::RealRecord {
        ValueRecord {
            name: "value".to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(fixture_offset(parent)),
            depth: 0,
            payload: RealPayload::Scalar {
                value: Real::from_bits(value.to_bits()),
            },
            offset,
        }
    }

    fn persistence(radii: &[f64]) -> Persistence {
        let mut objects = vec![
            object("features", "Sld_Features", None, ObjectPayload::Arrow),
            object(
                "feature",
                "first_feat_ptr",
                Some("features"),
                ObjectPayload::Arrow,
            ),
            object(
                "feature_type",
                "feat_type_ptr",
                Some("feature"),
                ObjectPayload::Arrow,
            ),
            object("full_data", "Sld_FullData", None, ObjectPayload::Arrow),
        ];
        let mut integer_values = vec![
            integer("feature", "id", 139, 1),
            integer("feature_type", "type", 913, 2),
        ];
        let mut real_values = Vec::new();
        let elements = radii
            .iter()
            .enumerate()
            .map(|(index, radius)| {
                let element = format!("dimension_{index}");
                let data = format!("dimension_data_{index}");
                objects.extend([
                    object(
                        &element,
                        "dim_array",
                        Some("dimension_array"),
                        ObjectPayload::Arrow,
                    ),
                    object(&data, "dim_dat_ptr", Some(&element), ObjectPayload::Arrow),
                ]);
                integer_values.extend([
                    integer(&element, "type", 8, 10 + index),
                    integer(&element, "dim_type", 3, 20 + index),
                    integer(&element, "feat_id", 139, 30 + index),
                ]);
                real_values.push(real(&data, *radius, 40 + index));
                element
            })
            .collect::<Vec<_>>();
        objects.push(object(
            "dimension_array",
            "dim_array",
            Some("full_data"),
            ObjectPayload::Array {
                dimensions: vec![u32::try_from(elements.len()).expect("test extent")],
                elements,
            },
        ));
        Persistence {
            real_values: crate::legacy::TypedValues {
                rows: real_values,
                unresolved_count: 0,
            },
            integer_values: crate::legacy::TypedValues {
                rows: integer_values,
                unresolved_count: 0,
            },
            objects,
            ..Persistence::default()
        }
    }

    fn topology(id: u32) -> CurveTopologyRow {
        CurveTopologyRow {
            id,
            type_byte: 0,
            feature_id: 139,
            directions: [1, 1],
            faces: [None, None],
            next_edges: [0, 0],
            offset: id as usize,
        }
    }

    #[test]
    fn joins_constant_round_radius_and_owned_edges() {
        let result = scan(&persistence(&[2.0, 2.0]), &[topology(7), topology(8)]);
        assert_eq!(result.rounds.len(), 1);
        assert_eq!(result.rounds[0].feature_id, 139);
        assert_eq!(
            result.rounds[0].radius,
            LegacyRoundRadius::Constant(
                cadmpeg_ir::scalar::PositiveReal::new(2.0).expect("positive radius")
            )
        );
        assert_eq!(result.rounds[0].edge_ids, Some(vec![7, 8]));
    }

    #[test]
    fn withholds_variable_round_radius() {
        let result = scan(&persistence(&[2.0, 3.0]), &[topology(7)]);
        assert_eq!(result.rounds[0].radius, LegacyRoundRadius::Ambiguous);
    }

    #[test]
    fn withholds_duplicate_owned_edge_identity() {
        let result = scan(&persistence(&[2.0]), &[topology(7), topology(7)]);
        assert_eq!(result.rounds[0].edge_ids, None);
    }

    #[test]
    fn ignores_non_round_dimension_rows() {
        let mut persistence = persistence(&[2.0]);
        persistence.integer_values.rows.retain(|record| {
            !(record.parent == Some(fixture_offset("dimension_0")) && record.name == "dim_type")
        });
        let result = scan(&persistence, &[]);
        assert_eq!(result.rounds[0].radius, LegacyRoundRadius::NotPresent);
    }

    #[test]
    fn ignores_persistence_without_feature_root() {
        assert!(scan(&Persistence::default(), &[]).rounds.is_empty());
    }

    #[test]
    fn legacy_feature_object_index_nodes_refuse_before_insertion() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy feature object index nodes");
    }

    #[test]
    fn legacy_feature_child_index_nodes_refuse_before_insertion() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy feature child index nodes");
    }

    #[test]
    fn legacy_feature_child_index_rows_refuse_before_vec_growth() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy feature child index rows");
    }

    #[test]
    fn legacy_dimension_rows_refuse_before_vec_growth() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy dimension rows");
    }

    #[test]
    fn legacy_dimension_identities_refuse_before_insertion() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy dimension identities");
    }

    #[test]
    fn legacy_round_edge_identities_refuse_before_insertion() {
        let fixture = persistence(&[2.0]);
        let edges = [topology(7), topology(8)];
        assert_eq!(scan(&fixture, &edges).rounds[0].edge_ids, Some(vec![7, 8]));
        assert_collection_refusal(&fixture, &edges, "creo legacy round edge identities");
    }

    #[test]
    fn legacy_round_edge_ids_refuse_before_vec_growth() {
        let fixture = persistence(&[2.0]);
        let edges = [topology(7), topology(8)];
        assert_eq!(scan(&fixture, &edges).rounds[0].edge_ids, Some(vec![7, 8]));
        assert_collection_refusal(&fixture, &edges, "creo legacy round edge IDs");
    }

    #[test]
    fn legacy_round_index_nodes_refuse_before_insertion() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy round index nodes");
    }

    #[test]
    fn legacy_ambiguous_round_ids_refuse_before_insertion() {
        let mut fixture = persistence(&[2.0]);
        fixture.objects.extend([
            object(
                "duplicate_feature",
                "next_feat_ptr",
                Some("features"),
                ObjectPayload::Arrow,
            ),
            object(
                "duplicate_type",
                "feat_type_ptr",
                Some("duplicate_feature"),
                ObjectPayload::Arrow,
            ),
        ]);
        fixture.integer_values.rows.extend([
            integer("duplicate_feature", "id", 139, 90),
            integer("duplicate_type", "type", 913, 91),
        ]);
        assert!(scan(&fixture, &[]).rounds.is_empty());
        assert_collection_refusal(&fixture, &[], "creo legacy ambiguous round IDs");
    }

    #[test]
    fn legacy_round_results_refuse_before_vec_growth() {
        let fixture = persistence(&[2.0]);
        assert_eq!(scan(&fixture, &[]).rounds.len(), 1);
        assert_collection_refusal(&fixture, &[], "creo legacy round results");
    }
}
