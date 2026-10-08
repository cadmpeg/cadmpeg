// SPDX-License-Identifier: Apache-2.0
//! Feature-owned joins from the legacy ASCII persistence graph.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

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

type ChildrenIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a ObjectRecord>>;
type IntegerIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealIndex<'a> = BTreeMap<(usize, &'a str), Vec<&'a legacy::RealRecord>>;

struct Index<'a> {
    objects: HashMap<usize, &'a ObjectRecord>,
    roots: HashMap<&'a str, Option<&'a ObjectRecord>>,
    children: ChildrenIndex<'a>,
    integers: IntegerIndex<'a>,
    reals: RealIndex<'a>,
}

impl<'a> Index<'a> {
    fn build(
        ctx: &DecodeContext<'_>,
        persistence: &'a Persistence,
    ) -> Result<Option<Self>, CodecError> {
        let mut objects = HashMap::new();
        let mut roots = HashMap::new();
        let mut children = BTreeMap::new();
        let mut input = persistence.objects.iter();
        while let Some(object) =
            ctx.next_charged(&mut input, "creo legacy feature object traversal")?
        {
            match ctx.entry_hash_map(
                &mut objects,
                object.offset,
                "creo legacy feature object index nodes",
            )? {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(object);
                }
                std::collections::hash_map::Entry::Occupied(_) => return Ok(None),
            }
            if object.parent.is_none() {
                ctx.entry_hash_map(
                    &mut roots,
                    object.name.as_str(),
                    "creo legacy feature root index",
                )?
                .and_modify(|root| *root = None)
                .or_insert(Some(object));
            }
            if let Some(parent) = object.parent {
                let rows = ctx
                    .entry_btree_map(
                        &mut children,
                        (parent, object.name.as_str()),
                        "creo legacy feature child index nodes",
                    )?
                    .or_insert_with(Vec::new);
                ctx.push_vec(rows, object, "creo legacy feature child index rows")?;
            }
        }
        let mut integers = BTreeMap::new();
        ctx.admit_iter(
            &persistence.integer_values.rows,
            "creo legacy feature integer traversal",
        )?;
        legacy::value_index(ctx, &persistence.integer_values.rows, &mut integers)?;
        let mut reals = BTreeMap::new();
        ctx.admit_iter(
            &persistence.real_values.rows,
            "creo legacy feature real traversal",
        )?;
        legacy::value_index(ctx, &persistence.real_values.rows, &mut reals)?;
        Ok(Some(Self {
            objects,
            roots,
            children,
            integers,
            reals,
        }))
    }

    fn root(
        &self,
        ctx: &DecodeContext<'_>,
        name: &str,
    ) -> Result<Option<&'a ObjectRecord>, CodecError> {
        Ok(ctx
            .get_hash_map(&self.roots, name, "creo legacy feature root lookup")?
            .copied()
            .flatten())
    }

    fn children(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &'static str,
    ) -> Result<&[&'a ObjectRecord], CodecError> {
        Ok(ctx
            .get_btree_map(
                &self.children,
                &(parent, name),
                "creo legacy feature child lookup",
            )?
            .map_or(&[], Vec::as_slice))
    }

    fn unique_child(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &'static str,
    ) -> Result<Option<&ObjectRecord>, CodecError> {
        Ok(match self.children(ctx, parent, name)? {
            [child] => Some(*child),
            _ => None,
        })
    }

    fn integer(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &str,
    ) -> Result<Option<i32>, CodecError> {
        let Some(records) = ctx.get_btree_map(
            &self.integers,
            &(parent, name),
            "creo legacy feature integer lookup",
        )?
        else {
            return Ok(None);
        };
        let [record] = records.as_slice() else {
            return Ok(None);
        };
        Ok(match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array(_) => None,
        })
    }

    fn real(
        &self,
        ctx: &DecodeContext<'_>,
        parent: usize,
        name: &str,
    ) -> Result<Option<f64>, CodecError> {
        let Some(records) = ctx.get_btree_map(
            &self.reals,
            &(parent, name),
            "creo legacy feature real lookup",
        )?
        else {
            return Ok(None);
        };
        let [record] = records.as_slice() else {
            return Ok(None);
        };
        Ok(match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array(_) => None,
        })
    }
}

#[derive(Default)]
struct EdgeGroup<'a> {
    rows: Vec<&'a CurveTopologyRow>,
    ids: HashSet<u32>,
    duplicate: bool,
}

/// Decode feature-owned round records from one legacy persistence graph.
pub(crate) fn scan(
    ctx: &DecodeContext<'_>,
    persistence: &Persistence,
    topology_rows: &[CurveTopologyRow],
) -> Result<LegacyFeatureScan, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo legacy feature scratch")?;
    let Some(index) = storage.with_storage(|| Index::build(ctx, persistence))? else {
        return Ok(LegacyFeatureScan::default());
    };
    let Some(features_root) = index.root(ctx, "Sld_Features")? else {
        return Ok(LegacyFeatureScan::default());
    };
    let mut rounds = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    let first = index.children(ctx, features_root.offset, "first_feat_ptr")?;
    let next = index.children(ctx, features_root.offset, "next_feat_ptr")?;
    let mut feature_nodes = first.iter().chain(next);
    while let Some(feature) = ctx.next_charged(&mut feature_nodes, "creo legacy feature nodes")? {
        let Some(feature_id) = index
            .integer(ctx, feature.offset, "id")?
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(feature_type) = index.unique_child(ctx, feature.offset, "feat_type_ptr")? else {
            continue;
        };
        if index.integer(ctx, feature_type.offset, "type")? != Some(ROUND_SCHEMA_CLASS) {
            continue;
        }
        match storage.with_storage(|| {
            ctx.entry_btree_map(&mut rounds, feature_id, "creo legacy round index nodes")
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(feature.offset);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(feature.offset);
                storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut ambiguous,
                        feature_id,
                        "creo legacy ambiguous round IDs",
                    )
                })?;
            }
        }
    }
    let result_count = rounds.len() - ambiguous.len();
    if result_count == 0 {
        return Ok(LegacyFeatureScan::default());
    }
    let radius_rows = storage.with_storage(|| full_data_dimension_rows(ctx, &index))?;
    let mut radii = HashMap::new();
    if let Some(rows) = &radius_rows {
        for row in ctx.admit_iter(rows, "creo legacy radius row traversal")? {
            if index.integer(ctx, row.offset, "type")? != Some(DIMENSION_TYPE)
                || index.integer(ctx, row.offset, "dim_type")? != Some(ROUND_DIMENSION_KIND)
            {
                continue;
            }
            let Some(feature_id) = index
                .integer(ctx, row.offset, "feat_id")?
                .and_then(|value| u32::try_from(value).ok())
            else {
                continue;
            };
            let radius = dimension_radius(ctx, row, &index)?;
            let entry = storage.with_storage(|| {
                ctx.entry_hash_map(&mut radii, feature_id, "creo legacy radius groups")
            })?;
            entry
                .and_modify(|first| {
                    let agrees = match (*first, radius) {
                        (LegacyRoundRadius::Constant(a), LegacyRoundRadius::Constant(b)) => {
                            a.get().to_bits() == b.get().to_bits()
                        }
                        _ => false,
                    };
                    if !agrees {
                        *first = LegacyRoundRadius::Ambiguous;
                    }
                })
                .or_insert(radius);
        }
    }
    let mut edges = HashMap::<u32, EdgeGroup<'_>>::new();
    for row in ctx.admit_iter(topology_rows, "creo legacy edge row traversal")? {
        let group = storage
            .with_storage(|| {
                ctx.entry_hash_map(&mut edges, row.feature_id, "creo legacy edge groups")
            })?
            .or_default();
        if group.duplicate {
            continue;
        }
        if !storage.with_storage(|| {
            ctx.insert_hash_set(&mut group.ids, row.id, "creo legacy round edge identities")
        })? {
            group.duplicate = true;
            continue;
        }
        storage
            .with_storage(|| ctx.push_vec(&mut group.rows, row, "creo legacy edge group rows"))?;
    }
    let mut visible_rounds = Vec::new();
    ctx.reserve_vec(
        &mut visible_rounds,
        result_count,
        "creo legacy round results",
    )?;
    for (feature_id, offset) in ctx.admit_iter(rounds, "creo legacy round result traversal")? {
        if ctx.contains_btree_set(
            &ambiguous,
            &feature_id,
            "creo legacy ambiguous round lookup",
        )? {
            continue;
        }
        let edge_ids = match edges.get(&feature_id) {
            Some(group) if !group.duplicate && !group.rows.is_empty() => Some(ctx.collect_vec(
                group.rows.iter().map(|row| row.id),
                "creo legacy round edge IDs",
            )?),
            _ => None,
        };
        visible_rounds.push(LegacyRoundFeature {
            feature_id,
            radius: radii
                .get(&feature_id)
                .copied()
                .unwrap_or(LegacyRoundRadius::NotPresent),
            edge_ids,
            offset,
        });
    }
    Ok(LegacyFeatureScan {
        rounds: visible_rounds,
    })
}

fn full_data_dimension_rows<'a>(
    ctx: &DecodeContext<'_>,
    index: &'a Index<'a>,
) -> Result<Option<Vec<&'a ObjectRecord>>, CodecError> {
    const ID_PREFIX: &str = "creo:legacy_ascii:object#";
    let Some(root) = index.root(ctx, "Sld_FullData")? else {
        return Ok(None);
    };
    let [array] = index.children(ctx, root.offset, "dim_array")? else {
        return Ok(None);
    };
    let ObjectPayload::Array {
        dimensions,
        elements,
        ..
    } = &array.payload
    else {
        return Ok(None);
    };
    let mut count = 1_u64;
    let mut dimensions = dimensions.iter();
    while let Some(dimension) = ctx.next_charged(&mut dimensions, "creo legacy dimension extent")? {
        let Some(product) = count.checked_mul(u64::from(*dimension)) else {
            return Ok(None);
        };
        count = product;
    }
    if usize::try_from(count).ok() != Some(elements.len()) {
        return Ok(None);
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    ctx.reserve_vec(&mut rows, elements.len(), "creo legacy dimension rows")?;
    let mut elements = elements.iter();
    while let Some(element_id) =
        ctx.next_charged(&mut elements, "creo legacy dimension row traversal")?
    {
        let Some(number) =
            ctx.strip_prefix(element_id, ID_PREFIX, "creo legacy dimension object prefix")?
        else {
            return Ok(None);
        };
        if number.is_empty()
            || (number.len() > 1 && number.starts_with('0'))
            || !number.as_bytes()[0].is_ascii_digit()
        {
            return Ok(None);
        }
        let Ok(offset) = ctx.parse_text::<usize>(number, "creo legacy dimension object offset")?
        else {
            return Ok(None);
        };
        if !ctx.insert_hash_set(&mut seen, offset, "creo legacy dimension identities")? {
            return Ok(None);
        }
        let Some(element) = index.objects.get(&offset).copied() else {
            return Ok(None);
        };
        if element.parent != Some(array.offset) || element.name != "dim_array" {
            return Ok(None);
        }
        rows.push(element);
    }
    Ok(Some(rows))
}

fn dimension_radius(
    ctx: &DecodeContext<'_>,
    row: &ObjectRecord,
    index: &Index<'_>,
) -> Result<LegacyRoundRadius, CodecError> {
    let Some(data) = index.unique_child(ctx, row.offset, "dim_dat_ptr")? else {
        return Ok(LegacyRoundRadius::Ambiguous);
    };
    Ok(index
        .real(ctx, data.offset, "value")?
        .and_then(PositiveReal::new)
        .map_or(LegacyRoundRadius::Ambiguous, LegacyRoundRadius::Constant))
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
        let cap = crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some(operation),
            |limit| scan_with_collection_limit(persistence, topology_rows, limit),
        );
        let refusal = scan_with_collection_limit(persistence, topology_rows, cap)
            .expect_err("collection boundary refuses before growth");
        assert!(matches!(refusal, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
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
                complete: true,
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
            offset: usize::try_from(id).expect("fixture index fits usize"),
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
    #[test]
    fn legacy_feature_joins_refuse_at_work_boundaries() {
        let persistence = persistence(&[2.0, 2.0]);
        let edges = [topology(7), topology(8)];
        crate::test_support::assert_work_boundaries(
            &[
                "creo legacy feature object traversal",
                "creo legacy feature integer traversal",
                "creo legacy feature real traversal",
                "creo legacy feature root lookup",
                "creo legacy feature child lookup",
                "creo legacy feature integer lookup",
                "creo legacy feature real lookup",
                "creo legacy feature nodes",
                "creo legacy dimension row traversal",
                "creo legacy dimension extent",
                "creo legacy dimension object offset",
                "creo legacy radius row traversal",
                "creo legacy edge row traversal",
                "creo legacy round result traversal",
            ],
            |ctx| scan_checked(ctx, &persistence, &edges),
        );
    }
}
