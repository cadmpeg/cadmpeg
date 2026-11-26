// SPDX-License-Identifier: Apache-2.0
use super::FastMap;
use crate::history_records::{
    AsmHistoricalBodyDesignLink, AsmHistoricalCarrierBinding, AsmHistoricalCoedge,
    AsmHistoricalCurveAxis, AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalEdgeSample,
    AsmHistoricalEnvelope, AsmHistoricalFaceSample, AsmHistoricalFaceSense,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPersistentSubentityTag, AsmHistoricalPlane,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalSurfaceAxis,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};
use std::collections::hash_map::Entry;

/// One bit per topology family.
type Families = u64;

/// Inline storage for the common single-row key, spilling to a heap vector
/// only when multiple rows share the same key.
enum Rows<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> Rows<T> {
    #[inline]
    fn push(&mut self, value: T) {
        match self {
            Self::Many(items) => items.push(value),
            Self::One(_) => {
                let Self::One(first) = std::mem::replace(self, Self::Many(Vec::new())) else {
                    return;
                };
                *self = Self::Many(vec![first, value]);
            }
        }
    }

    #[inline]
    fn as_slice(&self) -> &[T] {
        match self {
            Self::One(item) => std::slice::from_ref(item),
            Self::Many(items) => items.as_slice(),
        }
    }
}

impl<T: Clone> Rows<T> {
    #[inline]
    fn from_slice(rows: &[T]) -> Self {
        match rows {
            [single] => Self::One(single.clone()),
            _ => Self::Many(rows.to_vec()),
        }
    }

    #[inline]
    fn extend_from_slice(&mut self, rows: &[T]) {
        match self {
            Self::Many(items) => items.extend_from_slice(rows),
            Self::One(_) => {
                let Self::One(first) = std::mem::replace(self, Self::Many(Vec::new())) else {
                    return;
                };
                let mut items = Vec::with_capacity(1 + rows.len());
                items.push(first);
                items.extend_from_slice(rows);
                *self = Self::Many(items);
            }
        }
    }
}

fn group<T>(
    rows: impl IntoIterator<Item = T>,
    key: impl Fn(&T) -> i64,
    family: Families,
    families: &mut FastMap<i64, Families>,
) -> FastMap<i64, Rows<T>> {
    let rows = rows.into_iter();
    let mut groups = FastMap::<i64, Rows<T>>::default();
    groups.reserve(rows.size_hint().0);
    for row in rows {
        let slot = key(&row);
        *families.entry(slot).or_default() |= family;
        match groups.entry(slot) {
            Entry::Vacant(vacant) => {
                vacant.insert(Rows::One(row));
            }
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().push(row);
            }
        }
    }
    groups
}

fn ordered_rows<T>(rows: &[T], key: impl Fn(&T) -> i64) -> bool {
    rows.windows(2).all(|pair| key(&pair[0]) <= key(&pair[1]))
}

/// One replaced key: the families it held before the replacement and the
/// families the replacement has written.
struct KeyEdit {
    key: i64,
    present: Families,
    written: Families,
}

/// Overwrite the rows of each edited key in place. A key's first run replaces
/// its old rows and later runs append, so an unsorted part keeps every row.
fn write_family<T: Clone>(
    current: &mut FastMap<i64, Rows<T>>,
    edits: &mut [KeyEdit],
    rows: &[T],
    key: impl Fn(&T) -> i64,
    family: Families,
) {
    for run in rows.chunk_by(|left, right| key(left) == key(right)) {
        let slot = key(&run[0]);
        let Ok(at) = edits.binary_search_by_key(&slot, |edit| edit.key) else {
            continue;
        };
        if edits[at].written & family == 0 {
            current.insert(slot, Rows::from_slice(run));
            edits[at].written |= family;
        } else if let Some(target) = current.get_mut(&slot) {
            target.extend_from_slice(run);
        }
    }
}

macro_rules! indexed_topology {
    (entities { $($entity:ident = $ebit:literal: $ety:ty => $ekey:expr),* $(,)? }
     containers { $($container:ident = $cbit:literal: $cty:ty => $ckey:expr),* $(,)? }) => {
        /// Rows of each family grouped by key. `families` holds the families
        /// with rows at each slot, so a keyed edit visits only those.
        #[derive(Default)]
        pub(super) struct IndexedTopology {
            families: FastMap<i64, Families>,
            $($entity: FastMap<i64, Rows<$ety>>),*,
            $($container: FastMap<i64, Rows<$cty>>),*
        }
        impl IndexedTopology {
            const ENTITIES: Families = $((1 << $ebit))|*;
            const CONTAINERS: Families = $((1 << $cbit))|*;
        }
        const _: () = assert!(
            IndexedTopology::ENTITIES | IndexedTopology::CONTAINERS
                == (1 << [$($ebit,)* $($cbit,)*].len()) - 1,
            "family bits are distinct and dense"
        );
        impl From<AsmHistoricalTopology> for IndexedTopology {
            fn from(topology: AsmHistoricalTopology) -> Self {
                let mut families = FastMap::default();
                $(let $entity = group(topology.$entity, $ekey, 1 << $ebit, &mut families);)*
                $(let $container = group(topology.$container, $ckey, 1 << $cbit, &mut families);)*
                Self { families, $($entity),*, $($container),* }
            }
        }
        impl IndexedTopology {
            pub(super) fn ordered(topology: &AsmHistoricalTopology) -> bool {
                $(ordered_rows(&topology.$entity, $ekey))&&*
                    && $(ordered_rows(&topology.$container, $ckey))&&*
            }
            pub(super) fn bodies(&self) -> impl Iterator<Item = &i64> { self.bodies.keys() }
            pub(super) fn materialize(&self) -> AsmHistoricalTopology {
                let mut out = AsmHistoricalTopology::default();
                let mut keys: Vec<i64> = self.families.keys().copied().collect();
                keys.sort_unstable();
                for key in keys {
                    let Some(&present) = self.families.get(&key) else {
                        continue;
                    };
                    if present & Self::ENTITIES != 0 {
                        $(if present & (1 << $ebit) != 0 {
                            if let Some(rows) = self.$entity.get(&key) {
                                out.$entity.extend_from_slice(rows.as_slice());
                            }
                        })*
                    }
                    if present & Self::CONTAINERS != 0 {
                        $(if present & (1 << $cbit) != 0 {
                            if let Some(rows) = self.$container.get(&key) {
                                out.$container.extend_from_slice(rows.as_slice());
                            }
                        })*
                    }
                }
                out
            }
            pub(super) fn project(&self, keys: &[i64], containers: bool) -> AsmHistoricalTopology {
                let mut out = AsmHistoricalTopology::default();
                for key in keys {
                    let Some(&present) = self.families.get(key) else {
                        continue;
                    };
                    if containers {
                        $(if present & (1 << $cbit) != 0 {
                            if let Some(rows) = self.$container.get(key) {
                                out.$container.extend_from_slice(rows.as_slice());
                            }
                        })*
                    } else {
                        $(if present & (1 << $ebit) != 0 {
                            if let Some(rows) = self.$entity.get(key) {
                                out.$entity.extend_from_slice(rows.as_slice());
                            }
                        })*
                    }
                }
                out
            }
            pub(super) fn replace(&mut self, part: AsmHistoricalTopology, keys: &[i64], containers: bool) {
                let half = if containers { Self::CONTAINERS } else { Self::ENTITIES };
                let mut edits = keys
                    .iter()
                    .map(|&key| KeyEdit {
                        key,
                        present: self.families.get(&key).copied().unwrap_or_default(),
                        written: 0,
                    })
                    .collect::<Vec<_>>();
                if containers {
                    $(write_family(&mut self.$container, &mut edits, &part.$container, $ckey, 1 << $cbit);)*
                } else {
                    $(write_family(&mut self.$entity, &mut edits, &part.$entity, $ekey, 1 << $ebit);)*
                }
                for edit in edits {
                    let stale = edit.present & half & !edit.written;
                    if containers {
                        $(if stale & (1 << $cbit) != 0 {
                            self.$container.remove(&edit.key);
                        })*
                    } else {
                        $(if stale & (1 << $ebit) != 0 {
                            self.$entity.remove(&edit.key);
                        })*
                    }
                    let present = (edit.present & !half) | edit.written;
                    if present == edit.present {
                        continue;
                    }
                    if present == 0 {
                        self.families.remove(&edit.key);
                    } else {
                        self.families.insert(edit.key, present);
                    }
                }
            }
        }
    };
}

indexed_topology! {
    entities {
        faces = 0: i64 => |row: &i64| *row,
        loops = 1: i64 => |row: &i64| *row,
        coedges = 2: i64 => |row: &i64| *row,
        edges = 3: i64 => |row: &i64| *row,
        vertices = 4: i64 => |row: &i64| *row,
        points = 5: i64 => |row: &i64| *row,
        surfaces = 6: i64 => |row: &i64| *row,
        surface_radii = 7: AsmHistoricalSurfaceRadius => |row: &AsmHistoricalSurfaceRadius| row.surface,
        surface_cylinders = 8: AsmHistoricalCylinder => |row: &AsmHistoricalCylinder| row.surface,
        surface_planes = 9: AsmHistoricalPlane => |row: &AsmHistoricalPlane| row.surface,
        surface_axes = 10: AsmHistoricalSurfaceAxis => |row: &AsmHistoricalSurfaceAxis| row.surface,
        curves = 11: i64 => |row: &i64| *row,
        curve_axes = 12: AsmHistoricalCurveAxis => |row: &AsmHistoricalCurveAxis| row.curve,
        pcurves = 13: i64 => |row: &i64| *row,
        persistent_subentity_tags = 14: AsmHistoricalPersistentSubentityTag => |row: &AsmHistoricalPersistentSubentityTag| row.entity_ref,
        face_loops = 15: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        loop_coedges = 16: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        coedge_topology = 17: AsmHistoricalCoedge => |row: &AsmHistoricalCoedge| row.coedge,
        edge_vertices = 18: AsmHistoricalEdge => |row: &AsmHistoricalEdge| row.edge,
        face_surfaces = 19: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        face_senses = 37: AsmHistoricalFaceSense => |row: &AsmHistoricalFaceSense| row.face,
        edge_curves = 20: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        coedge_pcurves = 21: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        vertex_points = 22: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        point_positions = 23: AsmHistoricalPoint => |row: &AsmHistoricalPoint| row.point,
        edge_samples = 24: AsmHistoricalEdgeSample => |row: &AsmHistoricalEdgeSample| row.edge,
        face_envelopes = 34: AsmHistoricalEnvelope => |row: &AsmHistoricalEnvelope| row.entity,
        edge_envelopes = 35: AsmHistoricalEnvelope => |row: &AsmHistoricalEnvelope| row.entity,
        face_samples = 36: AsmHistoricalFaceSample => |row: &AsmHistoricalFaceSample| row.face,
    }
    containers {
        bodies = 25: i64 => |row: &i64| *row,
        regions = 26: i64 => |row: &i64| *row,
        shells = 27: i64 => |row: &i64| *row,
        body_regions = 28: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        region_shells = 29: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_faces = 30: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_wire_edges = 31: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_free_vertices = 32: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        body_design_links = 33: AsmHistoricalBodyDesignLink => |row: &AsmHistoricalBodyDesignLink| row.body,
    }
}

#[cfg(test)]
mod tests {
    use super::super::Slots;
    use super::*;
    use crate::history_records::{AsmHistoricalTransition, AsmStateSnapshot};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use std::collections::BTreeMap;

    #[test]
    fn keyed_chain_matches_full_diffs_with_repeats_and_tag_groups() {
        let tag = |ordinal| AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Face,
            entity_ref: 1,
            selector: 7,
            token: "face".into(),
            token_integer: 0,
            design_references: vec![8],
            ordinal,
        };
        let mut newer = AsmHistoricalTopology {
            faces: vec![1],
            surfaces: vec![10, 10, 20],
            curves: vec![40, 40],
            persistent_subentity_tags: vec![tag(0), tag(1)],
            ..Default::default()
        };
        let mut indexed = IndexedTopology::from(newer.clone());
        let mut newer_versions = BTreeMap::from([(1, 1), (10, 10), (20, 20), (40, 40)]);
        let keys = [1, 10, 40];
        let key_set = Slots::from(keys);
        for state_id in [2, 1] {
            let mut older = newer.clone();
            older.surfaces.retain(|slot| *slot != 10);
            older.surfaces.insert(0, 10);
            older.curves.clear();
            older.persistent_subentity_tags = vec![tag(2)];
            let mut older_versions = newer_versions.clone();
            for slot in &keys {
                older_versions.insert(*slot, *slot + state_id * 100);
            }
            let newer_part = indexed.project(&keys, false);
            indexed.replace(older.clone(), &keys, false);
            let older_part = indexed.project(&keys, false);
            assert_eq!(indexed.materialize(), older);
            let old = AsmStateSnapshot {
                state_id,
                topology: &older,
                versions: &older_versions,
            };
            let new = AsmStateSnapshot {
                state_id: state_id + 1,
                topology: &newer,
                versions: &newer_versions,
            };
            let older_fast: super::FastMap<i64, i64> =
                older_versions.iter().map(|(&k, &v)| (k, v)).collect();
            let newer_fast: super::FastMap<i64, i64> =
                newer_versions.iter().map(|(&k, &v)| (k, v)).collect();
            assert_eq!(
                super::super::transition_for_changes(
                    old.state_id,
                    &older_part,
                    &older_fast,
                    new.state_id,
                    &newer_part,
                    &newer_fast,
                    &key_set,
                ),
                AsmHistoricalTransition::between(Some(old), new),
            );
            newer = older;
            newer_versions = older_versions;
        }
    }
    #[test]
    fn sparse_edit_preserves_other_slots() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            surfaces: vec![10, 20, 30],
            ..Default::default()
        });
        let keys = [20];
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20]);
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![20, 20],
                ..Default::default()
            },
            &keys,
            false,
        );
        assert_eq!(indexed.materialize().surfaces, vec![10, 20, 20, 30]);
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20, 20]);
    }

    #[test]
    fn replace_keeps_every_row_of_an_unsorted_part() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            surfaces: vec![10, 20, 30],
            ..Default::default()
        });
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![20, 10, 20],
                ..Default::default()
            },
            &[10, 20],
            false,
        );
        assert_eq!(indexed.materialize().surfaces, vec![10, 20, 20, 30]);
    }

    #[test]
    fn replace_keeps_the_other_half_of_a_slot() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            faces: vec![5],
            surfaces: vec![5],
            bodies: vec![5],
            ..Default::default()
        });
        let keys = [5];
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![5],
                ..Default::default()
            },
            &keys,
            false,
        );
        let replaced = indexed.materialize();
        assert!(replaced.faces.is_empty());
        assert_eq!(replaced.surfaces, vec![5]);
        assert_eq!(replaced.bodies, vec![5]);
        indexed.replace(AsmHistoricalTopology::default(), &keys, true);
        assert!(indexed.project(&keys, true).bodies.is_empty());
        assert_eq!(indexed.project(&keys, false).surfaces, vec![5]);
    }
}
