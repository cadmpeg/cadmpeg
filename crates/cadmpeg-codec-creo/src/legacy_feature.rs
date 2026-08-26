// SPDX-License-Identifier: Apache-2.0
//! Feature-owned joins from the legacy ASCII persistence graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::curve::CurveTopologyRow;
use crate::legacy::{self, NumericPayload, ObjectPayload, ObjectRecord, Persistence};

const ROUND_SCHEMA_CLASS: i32 = 913;
const DIMENSION_TYPE: i32 = 8;
const ROUND_DIMENSION_KIND: i32 = 3;

/// The state of a legacy round's design-radius dimension join.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LegacyRoundRadius {
    /// The complete dimension table has no radius row for this feature.
    NotPresent,
    /// All feature-owned radius rows carry the same positive value.
    Constant(f64),
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

type ObjectIndex<'a> = BTreeMap<&'a str, &'a ObjectRecord>;
type ChildrenIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a ObjectRecord>>;
type IntegerIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a legacy::IntegerRecord>>;
type RealIndex<'a> = BTreeMap<(&'a str, &'a str), Vec<&'a legacy::RealRecord>>;

struct Index<'a> {
    objects: ObjectIndex<'a>,
    children: ChildrenIndex<'a>,
    integers: IntegerIndex<'a>,
    reals: RealIndex<'a>,
}

impl<'a> Index<'a> {
    fn build(persistence: &'a Persistence) -> Option<Self> {
        let mut objects = BTreeMap::new();
        let mut children = BTreeMap::new();
        for object in &persistence.objects {
            if objects.insert(object.id.as_str(), object).is_some() {
                return None;
            }
            if let Some(parent) = object.parent.as_deref() {
                children
                    .entry((parent, object.name.as_str()))
                    .or_insert_with(Vec::new)
                    .push(object);
            }
        }
        Some(Self {
            objects,
            children,
            integers: value_index(&persistence.integer_values),
            reals: value_index(&persistence.real_values),
        })
    }

    fn children(&self, parent: &str, name: &str) -> Vec<&'a ObjectRecord> {
        self.children
            .get(&(parent, name))
            .cloned()
            .unwrap_or_default()
    }

    fn unique_child(&self, parent: &str, name: &str) -> Option<&ObjectRecord> {
        let children = self.children(parent, name);
        let [child] = children.as_slice() else {
            return None;
        };
        Some(*child)
    }

    fn unique_integer_scalar(&self, parent: &str, name: &str) -> Option<i32> {
        let records = self.integers.get(&(parent, name))?;
        let [record] = records.as_slice() else {
            return None;
        };
        match &record.payload {
            NumericPayload::Scalar { value } => Some(*value),
            NumericPayload::Array { .. } => None,
        }
    }

    fn unique_real_scalar(&self, parent: &str, name: &str) -> Option<f64> {
        let records = self.reals.get(&(parent, name))?;
        let [record] = records.as_slice() else {
            return None;
        };
        match &record.payload {
            NumericPayload::Scalar { value } => Some(value.value()),
            NumericPayload::Array { .. } => None,
        }
    }
}

fn value_index<T>(
    records: &[legacy::ValueRecord<T>],
) -> BTreeMap<(&str, &str), Vec<&legacy::ValueRecord<T>>> {
    let mut index = BTreeMap::new();
    for record in records {
        if let Some(parent) = record.parent.as_deref() {
            index
                .entry((parent, record.name.as_str()))
                .or_insert_with(Vec::new)
                .push(record);
        }
    }
    index
}

/// Decode feature-owned round records from one legacy persistence graph.
pub(crate) fn scan(
    persistence: &Persistence,
    topology_rows: &[CurveTopologyRow],
) -> LegacyFeatureScan {
    let Some(index) = Index::build(persistence) else {
        return LegacyFeatureScan::default();
    };
    let Some(features_root) = unique_root(&index, "Sld_Features") else {
        return LegacyFeatureScan::default();
    };
    let radius_rows = full_data_dimension_rows(&index);
    let mut rounds = BTreeMap::new();
    let mut ambiguous_feature_ids = BTreeSet::new();
    let mut feature_nodes = index.children(features_root.id.as_str(), "first_feat_ptr");
    feature_nodes.extend(index.children(features_root.id.as_str(), "next_feat_ptr"));
    for feature in &feature_nodes {
        let Some(feature_id) = index
            .unique_integer_scalar(&feature.id, "id")
            .and_then(|value| u32::try_from(value).ok())
        else {
            continue;
        };
        let Some(feature_type_object) = index.unique_child(&feature.id, "feat_type_ptr") else {
            continue;
        };
        let Some(schema_class) = index.unique_integer_scalar(&feature_type_object.id, "type")
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
            edge_ids: unique_feature_edge_ids(topology_rows, feature_id),
            offset: feature.offset,
        };
        if rounds.insert(feature_id, round).is_some() {
            ambiguous_feature_ids.insert(feature_id);
        }
    }
    LegacyFeatureScan {
        rounds: rounds
            .into_iter()
            .filter_map(|(feature_id, round)| {
                (!ambiguous_feature_ids.contains(&feature_id)).then_some(round)
            })
            .collect(),
    }
}

fn unique_root<'a>(index: &'a Index<'a>, name: &str) -> Option<&'a ObjectRecord> {
    let roots = index
        .objects
        .values()
        .filter(|object| object.name == name && object.parent.is_none())
        .copied()
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return None;
    };
    Some(*root)
}

fn full_data_dimension_rows<'a>(index: &'a Index<'a>) -> Option<Vec<&'a ObjectRecord>> {
    let root = unique_root(index, "Sld_FullData")?;
    let arrays = index.children(root.id.as_str(), "dim_array");
    let [array] = arrays.as_slice() else {
        return None;
    };
    let array = *array;
    let ObjectPayload::Array {
        elements,
        complete: true,
        ..
    } = &array.payload
    else {
        return None;
    };
    let mut seen = BTreeSet::new();
    elements
        .iter()
        .map(|element_id| {
            seen.insert(element_id.as_str()).then_some(())?;
            let element = index.objects.get(element_id.as_str()).copied()?;
            (element.parent.as_deref() == Some(array.id.as_str()) && element.name == "dim_array")
                .then_some(element)
        })
        .collect()
}

fn round_radius(rows: &[&ObjectRecord], index: &Index<'_>, feature_id: u32) -> LegacyRoundRadius {
    let Ok(feature_id) = i32::try_from(feature_id) else {
        return LegacyRoundRadius::NotPresent;
    };
    let mut found = false;
    let mut values = Vec::new();
    for row in rows {
        let fields = (
            index.unique_integer_scalar(&row.id, "type"),
            index.unique_integer_scalar(&row.id, "dim_type"),
            index.unique_integer_scalar(&row.id, "feat_id"),
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
        let Some(dimension_data) = index.unique_child(&row.id, "dim_dat_ptr") else {
            return LegacyRoundRadius::Ambiguous;
        };
        let Some(value) = index.unique_real_scalar(&dimension_data.id, "value") else {
            return LegacyRoundRadius::Ambiguous;
        };
        if !value.is_finite() || value <= 0.0 {
            return LegacyRoundRadius::Ambiguous;
        }
        values.push(value);
    }
    if !found {
        return LegacyRoundRadius::NotPresent;
    }
    let Some(first) = values.first().copied() else {
        return LegacyRoundRadius::Ambiguous;
    };
    if values
        .iter()
        .all(|value| value.to_bits() == first.to_bits())
    {
        LegacyRoundRadius::Constant(first)
    } else {
        LegacyRoundRadius::Ambiguous
    }
}

fn unique_feature_edge_ids(rows: &[CurveTopologyRow], feature_id: u32) -> Option<Vec<u32>> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows.iter().filter(|row| row.feature_id == feature_id) {
        if !seen.insert(row.id) {
            return None;
        }
        ids.push(row.id);
    }
    (!ids.is_empty()).then_some(ids)
}

#[cfg(test)]
mod tests {
    use super::{scan, LegacyRoundRadius};
    use crate::curve::CurveTopologyRow;
    use crate::legacy::{
        IntegerPayload, ObjectPayload, ObjectRecord, Persistence, Real, RealPayload, ValueRecord,
    };

    fn object(id: &str, name: &str, parent: Option<&str>, payload: ObjectPayload) -> ObjectRecord {
        ObjectRecord {
            id: id.to_string(),
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: parent.map(str::to_string),
            depth: 0,
            payload,
            offset: 0,
        }
    }

    fn integer(
        parent: &str,
        name: &str,
        value: i32,
        offset: usize,
    ) -> crate::legacy::IntegerRecord {
        ValueRecord {
            id: format!("{parent}:{name}:{offset}"),
            name: name.to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(parent.to_string()),
            depth: 0,
            payload: IntegerPayload::Scalar { value },
            offset,
        }
    }

    fn real(parent: &str, value: f64, offset: usize) -> crate::legacy::RealRecord {
        ValueRecord {
            id: format!("{parent}:value:{offset}"),
            name: "value".to_string(),
            attribute_id: 0,
            scope_offset: 0,
            parent: Some(parent.to_string()),
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
            real_values,
            integer_values,
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
            faces: [0, 0],
            next_edges: [0, 0],
            offset: id as usize,
        }
    }

    #[test]
    fn joins_constant_round_radius_and_owned_edges() {
        let result = scan(&persistence(&[2.0, 2.0]), &[topology(7), topology(8)]);
        assert_eq!(result.rounds.len(), 1);
        assert_eq!(result.rounds[0].feature_id, 139);
        assert_eq!(result.rounds[0].radius, LegacyRoundRadius::Constant(2.0));
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
        persistence.integer_values.retain(|record| {
            !(record.parent.as_deref() == Some("dimension_0") && record.name == "dim_type")
        });
        let result = scan(&persistence, &[]);
        assert_eq!(result.rounds[0].radius, LegacyRoundRadius::NotPresent);
    }

    #[test]
    fn ignores_persistence_without_feature_root() {
        assert!(scan(&Persistence::default(), &[]).rounds.is_empty());
    }
}
