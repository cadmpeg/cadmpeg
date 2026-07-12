// SPDX-License-Identifier: Apache-2.0
//! Structural comparison of IR documents.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::annotations::{ExactnessNote, Provenance};
use crate::CadIr;

/// A modified entity and its differing top-level fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ModifiedEntity {
    /// Diff key of the entity, as produced by the arena's key function.
    pub id: String,
    /// Names of the top-level entity fields whose JSON-serialized values differ
    /// between the two documents.
    pub fields: Vec<String>,
}

/// Changes within one entity arena.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ArenaDiff {
    /// Arena name, matching the field name in [`crate::CadIr`] (e.g. `"faces"`).
    pub kind: String,
    /// Diff keys of entities present only in the right-hand document.
    pub added: Vec<String>,
    /// Diff keys of entities present only in the left-hand document.
    pub removed: Vec<String>,
    /// Entities present in both documents with at least one differing field.
    pub modified: Vec<ModifiedEntity>,
}

/// Changes within the sparse document annotation tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct AnnotationDiff {
    /// `(left, right)` interned stream tables, present only when they differ.
    pub stream_change: Option<(Vec<String>, Vec<String>)>,
    /// Provenance entries keyed by entity id.
    pub provenance: ArenaDiff,
    /// Exactness entries keyed by entity id.
    pub exactness: ArenaDiff,
}

impl AnnotationDiff {
    fn is_empty(&self) -> bool {
        self.stream_change.is_none()
            && self.provenance.added.is_empty()
            && self.provenance.removed.is_empty()
            && self.provenance.modified.is_empty()
            && self.exactness.added.is_empty()
            && self.exactness.removed.is_empty()
            && self.exactness.modified.is_empty()
    }
}

/// Structural changes between two IR documents.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct IrDiff {
    /// `(left, right)` units, present only when the two documents' units differ.
    pub unit_change: Option<(crate::units::Units, crate::units::Units)>,
    /// `(left, right)` tolerances, present only when the two documents' tolerances differ.
    pub tolerance_change: Option<(crate::units::Tolerances, crate::units::Tolerances)>,
    /// Changes to annotation streams, provenance, and exactness.
    pub annotations: AnnotationDiff,
    /// Per-arena diffs, one entry per arena compared.
    pub per_arena: Vec<ArenaDiff>,
}

impl IrDiff {
    /// Returns `true` when neither units, tolerances, nor any arena differ.
    pub fn is_empty(&self) -> bool {
        self.unit_change.is_none()
            && self.tolerance_change.is_none()
            && self.annotations.is_empty()
            && self.per_arena.iter().all(|arena| {
                arena.added.is_empty() && arena.removed.is_empty() && arena.modified.is_empty()
            })
    }
}

fn differing_fields<T: Serialize>(left: &T, right: &T) -> Vec<String> {
    let (Ok(Value::Object(left)), Ok(Value::Object(right))) =
        (serde_json::to_value(left), serde_json::to_value(right))
    else {
        return vec!["value".to_string()];
    };
    left.keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|key| left.get(*key) != right.get(*key))
        .cloned()
        .collect()
}

fn map_arena<T, F>(
    kind: impl Into<String>,
    left: &BTreeMap<String, T>,
    right: &BTreeMap<String, T>,
    fields: F,
) -> ArenaDiff
where
    T: PartialEq,
    F: Fn(&T, &T) -> Vec<String>,
{
    let removed = left
        .keys()
        .filter(|id| !right.contains_key(*id))
        .cloned()
        .collect();
    let added = right
        .keys()
        .filter(|id| !left.contains_key(*id))
        .cloned()
        .collect();
    let modified = left
        .iter()
        .filter_map(|(id, before)| {
            let after = right.get(id)?;
            (before != after).then(|| ModifiedEntity {
                id: id.clone(),
                fields: fields(before, after),
            })
        })
        .collect();
    ArenaDiff {
        kind: kind.into(),
        added,
        removed,
        modified,
    }
}

fn exactness_fields(left: &ExactnessNote, right: &ExactnessNote) -> Vec<String> {
    let mut fields = Vec::new();
    if left.entity != right.entity {
        fields.push("entity".to_string());
    }
    fields.extend(
        left.fields
            .keys()
            .chain(right.fields.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|field| left.fields.get(*field) != right.fields.get(*field))
            .map(|field| format!("fields.{field}")),
    );
    fields
}

fn annotation_diff(left: &CadIr, right: &CadIr) -> AnnotationDiff {
    AnnotationDiff {
        stream_change: (left.annotations.streams != right.annotations.streams).then(|| {
            (
                left.annotations.streams.clone(),
                right.annotations.streams.clone(),
            )
        }),
        provenance: map_arena(
            "annotations.provenance",
            &left.annotations.provenance,
            &right.annotations.provenance,
            differing_fields::<Provenance>,
        ),
        exactness: map_arena(
            "annotations.exactness",
            &left.annotations.exactness,
            &right.annotations.exactness,
            exactness_fields,
        ),
    }
}

fn arena<T, F>(kind: impl Into<String>, left: &[T], right: &[T], id: F) -> ArenaDiff
where
    T: PartialEq + Serialize,
    F: Fn(&T) -> String,
{
    let left: BTreeMap<_, _> = left.iter().map(|entity| (id(entity), entity)).collect();
    let right: BTreeMap<_, _> = right.iter().map(|entity| (id(entity), entity)).collect();
    let removed = left
        .keys()
        .filter(|id| !right.contains_key(*id))
        .cloned()
        .collect();
    let added = right
        .keys()
        .filter(|id| !left.contains_key(*id))
        .cloned()
        .collect();
    let modified = left
        .iter()
        .filter_map(|(id, before)| {
            let after = right.get(id)?;
            (*before != *after).then(|| ModifiedEntity {
                id: id.clone(),
                fields: differing_fields(*before, *after),
            })
        })
        .collect();
    ArenaDiff {
        kind: kind.into(),
        added,
        removed,
        modified,
    }
}

macro_rules! define_diff_arenas {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        fn diff_arenas(left: &CadIr, right: &CadIr) -> Vec<ArenaDiff> {
            vec![$(arena(
                stringify!($field),
                &left.model.$field,
                &right.model.$field,
                $key,
            )),*]
        }
    };
}
crate::document::arena_registry!(define_diff_arenas);

fn native_diff_arenas(left: &CadIr, right: &CadIr) -> Vec<ArenaDiff> {
    let namespaces = left
        .native
        .0
        .keys()
        .chain(right.native.0.keys())
        .collect::<std::collections::BTreeSet<_>>();
    namespaces
        .into_iter()
        .flat_map(|namespace| {
            let left_ns = left.native.namespace(namespace);
            let right_ns = right.native.namespace(namespace);
            let arenas = left_ns
                .into_iter()
                .flat_map(|value| value.arenas.keys())
                .chain(right_ns.into_iter().flat_map(|value| value.arenas.keys()))
                .collect::<std::collections::BTreeSet<_>>();
            arenas.into_iter().map(move |name| {
                arena(
                    format!("native.{namespace}.{name}"),
                    left_ns
                        .and_then(|value| value.arenas.get(name))
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    right_ns
                        .and_then(|value| value.arenas.get(name))
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                    |record| record.id.clone(),
                )
            })
        })
        .collect()
}

/// Compare units, tolerances, and every entity arena by stable entity ID.
pub fn diff(left: &CadIr, right: &CadIr) -> IrDiff {
    let unit_change =
        (left.units != right.units).then(|| (left.units.clone(), right.units.clone()));
    let tolerance_change =
        (left.tolerances != right.tolerances).then_some((left.tolerances, right.tolerances));
    let mut per_arena = diff_arenas(left, right);
    per_arena.extend(native_diff_arenas(left, right));
    IrDiff {
        unit_change,
        tolerance_change,
        annotations: annotation_diff(left, right),
        per_arena,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::diff;
    use crate::annotations::{ExactnessNote, Provenance};
    use crate::examples::unit_cube;
    use crate::provenance::Exactness;
    use std::collections::BTreeMap;

    #[test]
    fn detects_changes_in_all_document_dimensions() {
        let left = unit_cube();
        let mut right = left.clone();
        right.model.points[0].position.x += 1.0;
        right.model.loops.pop();
        right.model.coedges.pop();

        let result = diff(&left, &right);
        assert!(!result.is_empty());
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "points")
                .unwrap()
                .modified[0]
                .fields,
            ["position"]
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "loops")
                .unwrap()
                .removed
                .len(),
            1
        );
        assert_eq!(
            result
                .per_arena
                .iter()
                .find(|a| a.kind == "coedges")
                .unwrap()
                .removed
                .len(),
            1
        );
    }

    #[test]
    fn identical_documents_have_empty_diff() {
        let ir = unit_cube();
        assert!(diff(&ir, &ir).is_empty());
    }

    #[test]
    fn reports_provenance_offset_tag_and_stream_changes() {
        let mut left = unit_cube();
        let mut right = left.clone();
        let id = left.model.points[0].id.0.clone();
        left.annotations.streams = vec!["left-stream".to_string()];
        right.annotations.streams = vec!["unused-stream".to_string(), "right-stream".to_string()];
        left.annotations.provenance.insert(
            id.clone(),
            Provenance {
                stream: 0,
                offset: 10,
                tag: Some("left-tag".to_string()),
            },
        );
        right.annotations.provenance.insert(
            id.clone(),
            Provenance {
                stream: 1,
                offset: 20,
                tag: Some("right-tag".to_string()),
            },
        );

        let result = diff(&left, &right);
        assert_eq!(
            result.annotations.stream_change,
            Some((
                vec!["left-stream".to_string()],
                vec!["unused-stream".to_string(), "right-stream".to_string()]
            ))
        );
        assert_eq!(result.annotations.provenance.modified[0].id, id);
        assert_eq!(
            result.annotations.provenance.modified[0].fields,
            ["offset", "stream", "tag"]
        );
    }

    #[test]
    fn reports_field_exactness_changes_by_entity_id() {
        let mut left = unit_cube();
        let mut right = left.clone();
        let id = left.model.points[0].id.0.clone();
        left.annotations.exactness.insert(
            id.clone(),
            ExactnessNote {
                entity: Exactness::Inferred,
                fields: BTreeMap::from([("position.x".to_string(), Exactness::Derived)]),
            },
        );
        right.annotations.exactness.insert(
            id.clone(),
            ExactnessNote {
                entity: Exactness::Unknown,
                fields: BTreeMap::from([("position.x".to_string(), Exactness::ByteExact)]),
            },
        );

        let result = diff(&left, &right);
        assert_eq!(result.annotations.exactness.modified[0].id, id);
        assert_eq!(
            result.annotations.exactness.modified[0].fields,
            ["entity", "fields.position.x"]
        );
    }
}
