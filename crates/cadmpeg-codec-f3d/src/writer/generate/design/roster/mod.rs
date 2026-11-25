// SPDX-License-Identifier: Apache-2.0
//! The record roster of a design that writes a timeline (f3d.md
//! §3.1, "Records of a Base Feature design"): every record at the index its
//! role fixes, written from its layout template or its stated form, and the
//! archive members around them.

mod act;
mod bodies;
mod browser;
mod component;
pub(super) mod extrude;
mod fixed;
mod owned;
pub(super) mod planes;
mod revolve;
mod rows;
pub(super) mod segment;
pub(super) mod sketches;
pub(super) mod swept;
mod units;
pub(super) mod values;
mod views;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::record_residual_unavailable;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;

use super::frames::DesignRecords;
use crate::records::parameters::DesignParameterDiscriminator;
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::TypeRow;
use extrude::ExtrudeRecords;
use planes::PlaneRecords;
use revolve::RevolveRecords;
use segment::TypeTable;
use sketches::SketchRoster;
use swept::{Place, RolledBody};
use values::Values;

/// The `AssetSettings` record.
pub(super) const ASSET_SETTINGS: u32 = 0;
/// The Protein asset manager.
pub(super) const ASSET_MANAGER: u32 = 1;
/// The header-and-zero-bytes record `AssetSettings` names last.
const ZERO_RECORD: u32 = 2;
/// The component aggregate.
pub(super) const AGGREGATE: u32 = 3;
/// The component naming space.
pub(super) const NAMING_SPACE: u32 = 6;
/// The family discriminator of every parameter record the roster writes: the
/// value of every parameter record in a Design stream whose component naming
/// space is record [`NAMING_SPACE`] (f3d-open-items.md DR-911, DR-917).
pub(super) const PARAMETER_DISCRIMINATOR: DesignParameterDiscriminator =
    DesignParameterDiscriminator::Code6;
/// The design `Body` owner.
const DESIGN_OWNER: u32 = 7;
/// The sketch collection.
const SKETCHES: u32 = 10;
/// The parameter collection.
pub(super) const PARAMETER_COLLECTION: u32 = 17;
/// The alternate `Body` owner list.
const ALTERNATE_OWNERS: u32 = 19;
/// The record of type `F670456D-872C-4818-95E9-DE115B264ACA`.
pub(super) const RECORD_23: u32 = 23;
/// The origin container.
pub(super) const ORIGIN: u32 = 24;
/// The root instance.
pub(super) const ROOT_INSTANCE: u32 = 43;
/// The unit-system root.
const UNIT_ROOT: u32 = 46;
/// The named-view list.
const NAMED_VIEWS: u32 = 163;
/// The lighting environment.
const LIGHTING: u32 = 172;
/// The bodies root.
const BODIES_ROOT: u32 = 191;
/// The snapshot body-map companion of the design `Body` owner.
const DESIGN_COMPANION: u32 = 192;
/// The parameter `Body` owner.
const PARAMETER_OWNER: u32 = 193;
/// The snapshot body map.
const SNAPSHOT_MAP: u32 = 195;
/// The companion of the snapshot body map.
const SNAPSHOT_MAP_COMPANION: u32 = 196;
/// The body map.
const BODY_MAP: u32 = 197;
/// The timeline context.
pub(super) const CONTEXT: u32 = 198;
/// The `Base Feature` scope.
pub(super) const SCOPE: u32 = 209;
/// The ACT component record.
pub(super) const ACT_COMPONENT: u32 = 2;
/// The snapshot body-map companion of the parameter `Body` owner.
const PARAMETER_COMPANION: u32 = 219;
/// The first record index after the one-body roster: the records of further
/// bodies, then the user parameters.
const EXTENSION: u32 = 225;
/// The records of each body after the first.
const BODY_RECORDS: u32 = 10;
/// The roster records of the `Base Feature` item a design that writes an
/// `Extrude` in its place leaves out: the scope, its record-timestamp
/// tables, item-tail companion, tracked results and result owner, and the
/// body tracking entry.
const BASE_FEATURE_ITEM: [u32; 7] = [209, 210, 211, 212, 213, 214, 223];

/// The records of one result body.
#[derive(Clone, Copy)]
pub(super) struct BodyIndices {
    pub(super) design: u32,
    pub(super) parameter: u32,
    /// The Scene node, box and auxiliary record of the design
    /// body-reference record.
    pub(super) design_scene: [u32; 3],
    /// The same records of the parameter body-reference record.
    pub(super) parameter_scene: [u32; 3],
    pub(super) tracking: u32,
    pub(super) node: u32,
}

/// A geometry item the design writes: the item in place of its `Base
/// Feature`, or an item after the timeline marker. Its plan, its records
/// and its place in the timeline.
pub(in crate::writer::generate::design) struct ItemRecords<'a> {
    kind: ItemKind<'a>,
    pub(in crate::writer::generate::design) place: Place,
}

enum ItemKind<'a> {
    Extrude {
        plan: &'a super::extrude::ExtrudePlan<'a>,
        records: ExtrudeRecords,
    },
    Revolve {
        plan: &'a super::revolve::RevolvePlan<'a>,
        records: RevolveRecords,
    },
}

impl<'a> ItemRecords<'a> {
    /// The records of `item` from record `next` in creation order, with
    /// their type rows pushed to `rows`, and its first persistent identity
    /// and family ordinal from `place`. An item after the timeline marker
    /// adds the records of its rolled-back body, of the types `body_rows`.
    /// Returns the records and the next free index.
    fn allocate(
        item: &'a super::item::Item<'a>,
        next: u32,
        rows: &mut Vec<(u32, TypeRow)>,
        place: Place,
        body_rows: Option<&[TypeRow; 5]>,
    ) -> Result<(Self, u32), CodecError> {
        let (kind, mut next) = match item {
            super::item::Item::Extrude(plan) => {
                let (records, next) = ExtrudeRecords::allocate(plan, next, rows)?;
                (ItemKind::Extrude { plan, records }, next)
            }
            super::item::Item::Revolve(plan) => {
                let (records, next) = RevolveRecords::allocate(plan, next, rows)?;
                (ItemKind::Revolve { plan, records }, next)
            }
        };
        let rolled = match body_rows {
            Some(&[reference, scene_node, scene_box, auxiliary, node]) => {
                let mut take = swept::allocator(&mut next, rows);
                Some(RolledBody {
                    reference: take(reference)?,
                    scene: [take(scene_node)?, take(scene_box)?, take(auxiliary)?],
                    node: take(node)?,
                })
            }
            None => None,
        };
        Ok((
            Self {
                kind,
                place: Place { rolled, ..place },
            },
            next,
        ))
    }

    /// The item's scope.
    pub(in crate::writer::generate::design) fn scope(&self) -> u32 {
        match &self.kind {
            ItemKind::Extrude { records, .. } => records.scope.record,
            ItemKind::Revolve { records, .. } => records.scope.record,
        }
    }

    /// The number of `Sketch` items before the item in the timeline.
    pub(in crate::writer::generate::design) fn after(&self) -> usize {
        match &self.kind {
            ItemKind::Extrude { plan, .. } => plan.after,
            ItemKind::Revolve { plan, .. } => plan.after,
        }
    }

    /// The position of the profile's sketch in [`Context::sketches`].
    pub(in crate::writer::generate::design) fn sketch(&self) -> usize {
        match &self.kind {
            ItemKind::Extrude { plan, .. } => plan.sketch,
            ItemKind::Revolve { plan, .. } => plan.sketch,
        }
    }

    /// The feature-family name of the item.
    pub(in crate::writer::generate::design) fn family(&self) -> &'static str {
        match &self.kind {
            ItemKind::Extrude { .. } => extrude::FAMILY,
            ItemKind::Revolve { .. } => revolve::FAMILY,
        }
    }

    /// The timeline-context counter of the item's family.
    pub(in crate::writer::generate::design) fn counter(&self) -> &'static str {
        match &self.kind {
            ItemKind::Extrude { .. } => extrude::COUNTER,
            ItemKind::Revolve { .. } => revolve::COUNTER,
        }
    }

    /// The members the owner collection lists for the item, in its order.
    pub(in crate::writer::generate::design) fn owners(&self) -> Vec<u32> {
        match &self.kind {
            ItemKind::Extrude { records, .. } => records.owners(),
            ItemKind::Revolve { records, .. } => records.owners(),
        }
    }

    /// The members the `B65B5682` record lists for the item, in its order.
    pub(in crate::writer::generate::design) fn context_members(&self) -> Vec<u32> {
        match &self.kind {
            ItemKind::Extrude { records, .. } => records.context_members(),
            ItemKind::Revolve { records, .. } => records.context_members(),
        }
    }

    /// The parameter records the item owns.
    fn parameters(&self) -> Vec<u32> {
        match &self.kind {
            ItemKind::Extrude { records, .. } => records.parameters(),
            ItemKind::Revolve { records, .. } => records.parameters(),
        }
    }

    /// The input and dependent entries of the dependency table for the
    /// item, whose profile lies on the `Sketch` scope `profile`.
    pub(in crate::writer::generate::design) fn dependencies(
        &self,
        component: u32,
        profile: u32,
    ) -> (
        Vec<super::base_feature::Dependency>,
        Vec<super::base_feature::Dependency>,
    ) {
        match &self.kind {
            ItemKind::Extrude { records, .. } => records.dependencies(component, profile),
            ItemKind::Revolve { records, .. } => records.dependencies(component, profile),
        }
    }

    /// The base types of the item's records that the other ancestor lists
    /// leave out.
    fn ancestors(&self) -> &'static [TypeRow] {
        match &self.kind {
            ItemKind::Extrude { .. } => &super::types::EXTRUDE_ANCESTORS,
            ItemKind::Revolve { .. } => &super::types::REVOLVE_ANCESTORS,
        }
    }

    /// Every record of the item.
    fn write(&self, ctx: &Context<'_>, out: &mut DesignRecords) -> Result<(), CodecError> {
        #[cfg(test)]
        if let Some(error) = super::requested_item_failure() {
            return Err(error);
        }
        match &self.kind {
            ItemKind::Extrude { plan, records } => {
                extrude::write(ctx, plan, records, &self.place, out)
            }
            ItemKind::Revolve { plan, records } => {
                revolve::write(ctx, plan, records, &self.place, out)
            }
        }
    }

    /// The values the writer chooses where the specification leaves the
    /// rule open.
    fn losses(&self, ctx: &Context<'_>) -> Vec<LossNote> {
        match &self.kind {
            ItemKind::Extrude { plan, records } => {
                extrude::losses(ctx, plan, records, self.place.rolled.is_none())
            }
            ItemKind::Revolve { plan, records } => {
                revolve::losses(ctx, plan, records, self.place.rolled.is_none())
            }
        }
    }
}

/// The record indices of one design and the type row of each record.
pub(super) struct Roster<'a> {
    pub(super) bodies: Vec<BodyIndices>,
    pub(super) parameters: Vec<u32>,
    /// The records of the `Sketch` items, after the user parameters.
    pub(super) sketches: SketchRoster,
    /// The records of the `WorkPlane` items, in timeline order.
    pub(super) planes: Vec<PlaneRecords>,
    /// The geometry item, between the `Sketch` items before and after it.
    pub(super) item: Option<ItemRecords<'a>>,
    /// The geometry items after the timeline marker, in timeline order.
    pub(super) rolled: Vec<ItemRecords<'a>>,
    /// The value of the persistent-identity counter after every item.
    pub(super) next_identity: u64,
    /// Whether the timeline holds a `Base Feature` item: the design has a
    /// result body and no geometry item in its place.
    pub(super) base_feature: bool,
    rows: Vec<(u32, TypeRow)>,
}

/// The one-body roster row of record `index`.
fn row_of(index: u32) -> Result<TypeRow, CodecError> {
    rows::ONE_BODY
        .iter()
        .find(|(at, _)| *at == index)
        .map(|(_, row)| *row)
        .ok_or_else(|| CodecError::malformed(format_args!("roster row {index}")))
}

/// The records being allocated in timeline order, with the persistent
/// identity of the next item.
struct Allocation<'a, 'r> {
    next: u32,
    identity: u64,
    entries: &'r mut Vec<(u32, TypeRow)>,
    item: Option<ItemRecords<'a>>,
    rolled: Vec<ItemRecords<'a>>,
}

impl<'a> Allocation<'a, '_> {
    /// The geometry items of `geometry` after `position` `Sketch` items, each
    /// with whether it follows the timeline marker.
    fn items(
        &mut self,
        geometry: &[(&'a super::item::Item<'a>, bool)],
        position: usize,
    ) -> Result<(), CodecError> {
        let body_rows = [
            row_of(187)?,
            row_of(188)?,
            row_of(189)?,
            row_of(190)?,
            row_of(224)?,
        ];
        for (at, &(item, rolled)) in geometry.iter().enumerate() {
            if item.after() != position {
                continue;
            }
            let family = std::mem::discriminant(item);
            let ordinal = u32::try_from(
                geometry[..at]
                    .iter()
                    .filter(|(earlier, _)| std::mem::discriminant(*earlier) == family)
                    .count(),
            )
            .ok()
            .and_then(|earlier| earlier.checked_add(1))
            .ok_or_else(|| CodecError::NotImplemented("F3D family ordinal exceeds u32".into()))?;
            let (records, after) = ItemRecords::allocate(
                item,
                self.next,
                self.entries,
                Place {
                    identity: self.identity,
                    ordinal,
                    rolled: None,
                },
                rolled.then_some(&body_rows),
            )?;
            self.next = after;
            self.identity = self.identity.checked_add(2).ok_or_else(identity_overflow)?;
            if rolled {
                self.rolled.push(records);
            } else {
                self.item = Some(records);
            }
        }
        Ok(())
    }

    /// One `Sketch` or `WorkPlane` item's identity.
    fn one_identity(&mut self) -> Result<(), CodecError> {
        self.identity = self.identity.checked_add(1).ok_or_else(identity_overflow)?;
        Ok(())
    }
}

fn identity_overflow() -> CodecError {
    CodecError::NotImplemented("persistent identity overflow".into())
}

impl<'a> Roster<'a> {
    /// The one-body roster, with the ten records of each further body, one
    /// record of each user parameter, and then the records of each `Sketch`
    /// item, of the `WorkPlane` item before its first sketch, of the
    /// geometry `item` and of the `rolled` items after the timeline marker,
    /// in timeline order, at the indices from 225. The item replaces the
    /// `Base Feature` item. A design with no body writes neither the
    /// `Base Feature` item nor the records of a result body (f3d.md §3.1,
    /// "Records of a design whose timeline holds no body").
    pub(super) fn new(
        bodies: usize,
        parameters: usize,
        sketch_plan: &super::sketch::SketchPlan<'_>,
        item: Option<&'a super::item::Item<'a>>,
        rolled: &'a [super::item::Item<'a>],
    ) -> Result<Self, CodecError> {
        let sketches = &sketch_plan.items;
        let overflow = || CodecError::NotImplemented("Base Feature record index overflow".into());
        let mut entries = rows::ONE_BODY.to_vec();
        let base_feature = item.is_none() && bodies > 0;
        if !base_feature {
            entries.retain(|(index, _)| !BASE_FEATURE_ITEM.contains(index));
        }
        let first = BodyIndices {
            design: 187,
            parameter: 215,
            design_scene: [188, 189, 190],
            parameter_scene: [220, 221, 222],
            tracking: 223,
            node: 224,
        };
        let mut indices = Vec::new();
        if bodies == 0 {
            let body_records = [
                first.design,
                first.parameter,
                first.tracking,
                first.node,
                DESIGN_COMPANION,
                PARAMETER_COMPANION,
            ]
            .into_iter()
            .chain(first.design_scene)
            .chain(first.parameter_scene)
            .chain(rolled.is_empty().then_some(BODIES_ROOT))
            .collect::<Vec<_>>();
            entries.retain(|(index, _)| !body_records.contains(index));
        } else {
            indices.push(first);
        }
        let mut next = EXTENSION;
        for _ in 1..bodies {
            let start = next;
            let at = |offset: u32| start.checked_add(offset).ok_or_else(overflow);
            let body = BodyIndices {
                design: at(0)?,
                parameter: at(1)?,
                design_scene: [at(2)?, at(3)?, at(4)?],
                parameter_scene: [at(5)?, at(6)?, at(7)?],
                tracking: at(8)?,
                node: at(9)?,
            };
            for (record, model) in [
                (body.design, 187),
                (body.parameter, 215),
                (body.design_scene[0], 188),
                (body.design_scene[1], 189),
                (body.design_scene[2], 190),
                (body.parameter_scene[0], 220),
                (body.parameter_scene[1], 221),
                (body.parameter_scene[2], 222),
                (body.tracking, 223),
                (body.node, 224),
            ] {
                entries.push((record, row_of(model)?));
            }
            indices.push(body);
            next = start.checked_add(BODY_RECORDS).ok_or_else(overflow)?;
        }
        let mut parameter_indices = Vec::with_capacity(parameters);
        for _ in 0..parameters {
            entries.push((next, super::types::PARAMETER));
            parameter_indices.push(next);
            next = next.checked_add(1).ok_or_else(overflow)?;
        }
        let geometry = item
            .into_iter()
            .map(|item| (item, false))
            .chain(rolled.iter().map(|item| (item, true)))
            .collect::<Vec<_>>();
        let mut roster = SketchRoster::default();
        let mut planes = Vec::<PlaneRecords>::new();
        let mut allocation = Allocation {
            next,
            identity: super::base_feature::FIRST_IDENTITY,
            entries: &mut entries,
            item: None,
            rolled: Vec::new(),
        };
        for (position, planned) in sketches.iter().enumerate() {
            allocation.items(&geometry, position)?;
            for _ in sketch_plan
                .planes
                .iter()
                .filter(|plane| plane.first == position)
            {
                let (records, after) =
                    PlaneRecords::allocate(position, allocation.next, allocation.entries)?;
                planes.push(records);
                allocation.next = after;
                allocation.one_identity()?;
            }
            let plane_scope = match planned.support {
                super::sketch::Support::Origin(_) => None,
                super::sketch::Support::Construction(index) => Some(
                    planes
                        .get(index)
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!("construction plane {index}"))
                        })?
                        .scope,
                ),
            };
            allocation.next =
                roster.allocate(planned, plane_scope, allocation.next, allocation.entries)?;
            allocation.one_identity()?;
        }
        allocation.items(&geometry, sketches.len())?;
        let Allocation {
            identity,
            item: item_records,
            rolled: rolled_records,
            ..
        } = allocation;
        if rolled_records.len() != rolled.len() {
            return Err(CodecError::malformed(format_args!(
                "{} of {} rolled-back items stand in the timeline",
                rolled_records.len(),
                rolled.len()
            )));
        }
        // The `Base Feature` takes one identity.
        let next_identity = if base_feature {
            identity.checked_add(1).ok_or_else(identity_overflow)?
        } else {
            identity
        };
        Ok(Self {
            bodies: indices,
            parameters: parameter_indices,
            sketches: roster,
            planes,
            item: item_records,
            rolled: rolled_records,
            next_identity,
            base_feature,
            rows: entries,
        })
    }

    /// The geometry items in timeline order: the item in place of the
    /// `Base Feature`, then the items after the timeline marker.
    pub(super) fn geometry(&self) -> impl Iterator<Item = &ItemRecords<'a>> {
        self.item.iter().chain(&self.rolled)
    }

    pub(super) fn rows(&self) -> &[(u32, TypeRow)] {
        &self.rows
    }

    /// Whether the roster writes record `index`.
    fn writes(&self, index: u32) -> bool {
        self.rows.iter().any(|&(at, _)| at == index)
    }

    /// The next-entity-ID counter: one above every record index.
    pub(super) fn next_entity(&self) -> u64 {
        self.rows
            .iter()
            .map(|&(index, _)| u64::from(index) + 1)
            .max()
            .unwrap_or(0)
    }
}

/// One result body as the roster writes it.
pub(super) struct RosterBody<'a> {
    pub(super) id: &'a BodyId,
    /// The sum of the body's face and edge counts.
    pub(super) entities: usize,
    pub(super) sheet: bool,
    pub(super) node_guid: String,
    pub(super) parameter_guid: String,
}

/// What every record writer of one design reads.
pub(super) struct Context<'a> {
    pub(super) table: TypeTable,
    pub(super) roster: Roster<'a>,
    pub(super) values: Values,
    pub(super) bodies: Vec<RosterBody<'a>>,
    /// The `Sketch` items, in the order of [`Roster::sketches`].
    pub(super) sketches: &'a [super::sketch::PlannedSketch<'a>],
    /// The construction planes, in the order of [`Roster::planes`].
    pub(super) planes: &'a [super::sketch::PlannedPlane<'a>],
    pub(super) timeline_value: ContextValue,
    /// The position in [`Self::sketches`] of the first `Sketch` item after
    /// the timeline marker; a `WorkPlane` item before it is after the marker
    /// too. `sketches.len()` where the marker stands after the last item.
    pub(super) marker: usize,
}

/// The timeline context value.
pub(super) enum ContextValue {
    /// The value f3d.md §3.1 states for the design ("Records the timeline
    /// context names", "Design records of Sketch items").
    Stated(u64),
    /// The largest source ordinal of an owned parameter, for a design
    /// outside the f3d.md rules, with what puts it outside them.
    Chosen { value: u64, outside: String },
}

impl ContextValue {
    /// The value of a design whose parameters are named `names` and whose
    /// timeline holds `planes`, `sketches` and the geometry `item`, or a
    /// `Base Feature` without `item`, and the `rolled` items after the
    /// timeline marker.
    pub(super) fn of<'n>(
        names: impl IntoIterator<Item = &'n str>,
        sketches: &[super::sketch::PlannedSketch<'_>],
        planes: &[super::sketch::PlannedPlane<'_>],
        item: Option<&super::item::Item<'_>>,
        rolled: &[super::item::Item<'_>],
    ) -> Self {
        let largest = || {
            let dimensions = sketches
                .iter()
                .flat_map(|sketch| &sketch.dimensions)
                .map(|dimension| dimension.parameter.ordinal);
            let planes = planes
                .iter()
                .filter_map(|plane| plane.parameters.as_ref())
                .flat_map(|parameters| parameters.iter().map(|parameter| parameter.ordinal));
            let owned = item
                .into_iter()
                .chain(rolled)
                .flat_map(|item| item.owned().into_iter().map(|owned| owned.ordinal));
            dimensions
                .chain(planes)
                .chain(owned)
                .max()
                .map_or(0, u64::from)
        };
        if !planes.is_empty() {
            return Self::Chosen {
                value: largest(),
                outside: "the timeline holds WorkPlane items".to_owned(),
            };
        }
        if !rolled.is_empty() {
            return Self::Chosen {
                value: largest(),
                outside: "the timeline holds items after the timeline marker".to_owned(),
            };
        }
        if let Some(super::item::Item::Revolve(_)) = item {
            return Self::Chosen {
                value: largest(),
                outside: "the timeline holds a Revolve item".to_owned(),
            };
        }
        if item.is_none() {
            if sketches.is_empty() {
                return Self::Stated(0);
            }
            return Self::Chosen {
                value: largest(),
                outside: "the timeline holds Sketch items and a Base Feature".to_owned(),
            };
        }
        let mut largest_number = 0;
        let mut renamed = Vec::new();
        for name in names {
            match owned_number(name) {
                Some(number) => largest_number = largest_number.max(number),
                None => renamed.push(name),
            }
        }
        if renamed.is_empty() {
            return Self::Stated(largest_number);
        }
        Self::Chosen {
            value: largest(),
            outside: format!("parameters not named d<n>: {}", renamed.join(" ")),
        }
    }

    pub(super) fn value(&self) -> u64 {
        match self {
            Self::Stated(value) | Self::Chosen { value, .. } => *value,
        }
    }
}

/// `n` of a parameter name `d<n>`.
fn owned_number(name: &str) -> Option<u64> {
    let number = name.strip_prefix('d')?.parse::<u64>().ok()?;
    (format!("d{number}") == name).then_some(number)
}

impl Context<'_> {
    /// The class tag of the type `guid`.
    pub(super) fn class(&self, guid: &str) -> Result<DesignClassTag, CodecError> {
        self.table.class(guid)
    }

    /// The class tag of the type of record `index`.
    pub(super) fn class_of(&self, index: u32) -> Result<DesignClassTag, CodecError> {
        let row = self
            .roster
            .rows
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, row)| row)
            .ok_or_else(|| CodecError::malformed(format_args!("no roster record {index}")))?;
        self.table.class(row.guid)
    }

    /// Record `index` from its layout template with `binds`.
    pub(super) fn templated(
        &self,
        out: &mut DesignRecords,
        index: u32,
        template: &[u8],
        unstated: &[(usize, usize)],
        binds: &[(usize, super::frames::Bind<'_>)],
    ) -> Result<(), CodecError> {
        out.templated(&self.class_of(index)?, index, template, unstated, binds)?;
        Ok(())
    }

    /// The parameter records whose tails name the parameter collection, in
    /// ascending record index: the user parameters and the parameters the
    /// timeline items own.
    pub(super) fn collected_parameters(&self) -> Vec<u32> {
        let mut parameters = self.roster.parameters.clone();
        for sketch in &self.roster.sketches.items {
            parameters.extend(sketch.parameters());
        }
        for plane in &self.roster.planes {
            parameters.extend([plane.along[0], plane.extra[0]]);
        }
        for item in self.roster.geometry() {
            parameters.extend(item.parameters());
        }
        parameters.sort_unstable();
        parameters
    }

    /// The timeline item whose scope the browser node of a result body
    /// names: the geometry item's scope, else the `Base Feature` scope.
    pub(super) fn item_scope(&self) -> u32 {
        self.roster.item.as_ref().map_or(SCOPE, ItemRecords::scope)
    }

    /// The number of result bodies as a u32.
    pub(super) fn body_count(&self) -> Result<u32, CodecError> {
        count(self.bodies.len())
    }
}

/// The Design type table of `roster`: its records' rows and the rows with an
/// empty entity-ID run, with the sheet row where a result body is a sheet
/// and the ancestors of the `Sketch` records where the design writes one.
pub(super) fn type_table(roster: &Roster, sheet: bool) -> Result<TypeTable, CodecError> {
    let mut known = rows::EMPTY_ROWS.to_vec();
    if sheet {
        known.push(rows::SHEET_ROW);
    }
    if !roster.sketches.is_empty() {
        known.extend(super::types::SKETCH_ANCESTORS);
    }
    for item in roster.geometry() {
        known.extend(item.ancestors());
    }
    if roster.sketches.has_dimensions() {
        known.extend(super::types::DIMENSION_ANCESTORS);
    }
    if !roster.planes.is_empty() {
        known.extend(super::types::WORKPLANE_ANCESTORS);
    }
    TypeTable::new(roster.rows(), &known)
}

/// The records of the Design segment that store an LP-ASCII name after four
/// zero bytes.
pub(super) const NAMED: [u32; 13] = [0, 1, 4, 43, 44, 45, 46, 161, 177, 184, 185, 186, 191];

pub(super) fn count(length: usize) -> Result<u32, CodecError> {
    u32::try_from(length).map_err(|_| CodecError::NotImplemented("Design run exceeds u32".into()))
}

/// The members of the design folder: `(path, bytes)` with the path relative
/// to the folder.
pub(super) type Members = Vec<(String, Vec<u8>)>;

/// The Design records of `ctx` with `records`, the records the timeline
/// writer wrote, as the design folder's members: the segments, and the
/// history stream `smbh` and the snapshot stream `smb` of `streams` of a
/// design with a result body; and the losses of the values the
/// specification leaves open.
pub(super) fn archive(
    ctx: &Context<'_>,
    mut records: DesignRecords,
    streams: Option<(&[u8], &[u8])>,
) -> Result<(Members, Vec<LossNote>), CodecError> {
    if streams.is_some() == ctx.roster.bodies.is_empty() {
        return Err(CodecError::malformed(format_args!(
            "a design with {} result bodies and {} B-rep streams",
            ctx.roster.bodies.len(),
            if streams.is_some() { "its" } else { "no" }
        )));
    }
    fixed::write(ctx, &mut records)?;
    units::write(ctx, &mut records)?;
    views::write(ctx, &mut records)?;
    component::write(ctx, &mut records)?;
    bodies::write(ctx, &mut records)?;
    planes::write(ctx, &mut records)?;
    sketches::write(ctx, ctx.sketches, &mut records)?;
    for item in ctx.roster.geometry() {
        item.write(ctx, &mut records)?;
    }
    let records = records.sorted()?;
    let design = segment::Segment {
        kind: segment::SegmentKind::Design,
        table: &ctx.table,
        records: &records,
        named: NAMED
            .into_iter()
            .filter(|&index| ctx.roster.writes(index))
            .collect(),
        next_entity: ctx.roster.next_entity(),
    };
    let act = act::Act::new(ctx)?;
    let browser = browser::Browser::new()?;
    let mut members = Vec::new();
    for segment in [design, act.segment(), browser.segment()] {
        members.extend(segment.members()?);
    }
    if let Some((smbh, smb)) = streams {
        let [history, snapshot] = bodies::stream_basenames(ctx);
        members.push((
            format!("Breps.BlobParts/{history}"),
            super::history::with_header(smbh, HISTORY_FLAGS, HISTORY_SCALE)?,
        ));
        members.push((format!("Breps.BlobParts/{snapshot}"), smb.to_vec()));
    }
    for basename in fixed::configuration_basenames(ctx) {
        members.push((
            format!("DesignConfigurationTable.BlobParts/{basename}"),
            EMPTY_CONFIGURATION.to_vec(),
        ));
    }
    members.push((IMAGES.to_owned(), Vec::new()));
    Ok((members, losses(ctx)))
}

/// The flags word of the history stream: a history partition, save-format
/// revision 1.
const HISTORY_FLAGS: u64 = 3;
/// The `scale` word of the history stream of a Base Feature design.
const HISTORY_SCALE: f64 = 40.0;
/// The bytes of the configuration table and of the configuration rule set
/// of a design with no configuration.
const EMPTY_CONFIGURATION: &[u8] = b"{}";
/// The empty directory of the document's images.
pub(super) const IMAGES: &str = "Images.BlobParts/";

/// The reason of the note that charges the record-timestamp tables.
pub(super) const FIXED_CLOCK: &str = "writes fixed-clock (2026-01-01 UTC) authoring timestamps in the record-timestamp tables: the document records no authoring clock";

/// The values this writer chooses where the specification leaves the rule
/// open.
fn losses(ctx: &Context<'_>) -> Vec<LossNote> {
    let mut notes = Vec::new();
    if ctx.roster.base_feature {
        notes.extend(super::base_feature::item_tail_loss(SCOPE));
    }
    if let Some(item) = &ctx.roster.item {
        notes.extend(item.losses(ctx));
    }
    for item in &ctx.roster.rolled {
        notes.extend(item.losses(ctx));
    }
    notes.extend(record_residual_unavailable(
        "F3D",
        "writes these items after the timeline marker with no history state in the tail that f3d.md §3.1 gives such a feature (\"In every form, the state bytes\"), and the rolled-back body of each as a body-reference record that the bodies root lists last, whose Scene records store the box of the parameter-side Scene records, and a hidden browser node; f3d-open-items.md DR-1032 leaves open the result-body records of a rolled-back feature that makes a body and holds no state",
        ctx.roster.rolled.iter().map(|item| {
            let body = item.place.rolled.map_or_else(String::new, |body| {
                format!(
                    "; body-reference record {}, Scene records {} to {}, browser node {}",
                    body.reference, body.scene[0], body.scene[2], body.node
                )
            });
            format!("record {} ({} scope{body})", item.scope(), item.family())
        }),
    ));
    notes.extend(sketches::dimensions::losses(ctx));
    let planes = ctx
        .roster
        .planes
        .iter()
        .zip(ctx.planes)
        .filter(|(_, planned)| planned.first >= ctx.marker)
        .map(|(records, _)| format!("record {} (WorkPlane scope: its item tail)", records.scope));
    let sketches = ctx
        .roster
        .sketches
        .items
        .iter()
        .skip(ctx.marker)
        .map(|records| format!("record {} (Sketch scope: its item tail)", records.scope));
    notes.extend(record_residual_unavailable(
        "F3D",
        "writes the item tail of these items after the timeline marker as the tail of a built item with the rolled-back flag s1 = 1: no-result flag s0 = 0, state bytes 00 00 00 and null history-state identities. This breaks the rule of f3d.md §3.1 (\"In every form, the state bytes\") that a rolled-back feature that holds no state stores s0 = 1; f3d-open-items.md DR-1032 leaves open which other states set s0",
        planes.chain(sketches),
    ));
    if let ContextValue::Chosen { value, outside } = &ctx.timeline_value {
        notes.extend(record_residual_unavailable(
            "F3D",
            "writes the timeline context value where f3d.md §3.1 states it (\"Records the timeline context names\" and \"Design records of Sketch items\"), and f3d-open-items.md DR-882 leaves the value of other designs open",
            [format!("record {CONTEXT} (timeline context: context value {value}, the largest source ordinal of an owned parameter; {outside})")],
        ));
    }
    notes.extend(record_residual_unavailable(
        "F3D",
        &format!(
            "writes these user parameters with family discriminator {}, the value of every parameter record in a Design stream whose component naming space is record {NAMING_SPACE} (f3d-open-items.md DR-911)",
            PARAMETER_DISCRIMINATOR.code()
        ),
        ctx.roster
            .parameters
            .iter()
            .map(|record| format!("record {record} (user parameter)")),
    ));
    notes.extend(record_residual_unavailable(
        "F3D",
        FIXED_CLOCK,
        ctx.roster
            .rows()
            .iter()
            .filter(|(_, row)| row.guid == super::types::RECORD_TIMESTAMPS.guid)
            .map(|(record, _)| format!("record {record} (record-timestamp table)")),
    ));
    if let Some(fallback) = ctx.values.unit_system_fallback {
        let label = match fallback {
            values::UnitFallback::Unmodeled(unit) => format!("length unit {unit:?}"),
            values::UnitFallback::Undeclared => "no declared length unit".to_owned(),
        };
        notes.extend(record_residual_unavailable(
            "F3D",
            &format!(
                "writes MmMKS (unit system 67) for {label}: Fusion's custom unit-system table models only mm, cm, m, in and ft"
            ),
            ["record 0 (AssetSettings)".to_owned()],
        ));
    }
    notes.extend(record_residual_unavailable(
        "F3D",
        "fits the HOME camera, the AssetSettings view blocks and the scene radius to the body boxes by a rule f3d-open-items.md DR-896 leaves open",
        [
            "record 0 (AssetSettings)".to_owned(),
            "record 170 (HOME camera)".to_owned(),
        ],
    ));
    notes.extend(record_residual_unavailable(
        "F3D",
        "writes no display scene; f3d-open-items.md DR-914 leaves its layout open",
        ["OGS.BlobFolder".to_owned()],
    ));
    notes.extend(record_residual_unavailable(
        "F3D",
        "writes no Protein package for the asset manager (record 1) to name; f3d-open-items.md DR-905 leaves the text of its schema documents open and protein.md states no byte form of its value-block carriers",
        [format!(
            "ProteinAssets.BlobParts/{}",
            component::Assets::new(ctx).package
        )],
    ));
    notes.extend(record_residual_unavailable(
        "F3D",
        "writes these further or sheet result bodies by a record allocation and owner form that f3d-open-items.md DR-915 leaves open",
        ctx.bodies
            .iter()
            .enumerate()
            .filter(|(position, body)| *position > 0 || body.sheet)
            .map(|(_, body)| body.id.to_string()),
    ));
    notes.extend(record_residual_unavailable(
        "F3D",
        "stores the box of each body's vertex points as its axis-aligned bounding box in the Scene records, the HOME camera and the scene radius; a curved face can extend past its vertices",
        ctx.bodies.iter().map(|body| body.id.to_string()),
    ));
    let item = ctx
        .roster
        .item
        .as_ref()
        .map_or("Base Feature", ItemRecords::family);
    if !ctx.roster.sketches.is_empty() {
        notes.extend(record_residual_unavailable(
            "F3D",
            &format!(
                "writes the Sketch items before the {item} with these records and bytes by the majority of Fusion's Sketch designs; f3d-open-items.md DR-901 leaves their rule open"
            ),
            [
                format!("record 199 (owner collection: the order of the Sketch and {item} members)"),
                "record 200 (B65B5682: the order of its members)".to_owned(),
                "record 201 (dependency table: the order of its entries)".to_owned(),
                "record 203 (persistent-id table of Sketch items before the timeline's geometry item)".to_owned(),
                "Sketch scopes (P = 0, F = 1)".to_owned(),
                "sketch containers (word 1, sketch word 7, no base record)".to_owned(),
                "sketch points (flags and state by role)".to_owned(),
                "sketch curves (curve flag A, EntityGenesis)".to_owned(),
                "point and curve identities (`pt_tag`, `crv_primary_id`): one sequence from 100 across the sketches, each sketch's origin point, then its points, then its curves".to_owned(),
                format!("record {SKETCHES} (sketch collection: `crv_tag_id_idx`, one more than the largest point or curve identity)"),
                "placement tracking flags (01 01 00)".to_owned(),
                format!(
                    "Sketch scope tails, placements and placement paths (evaluation epoch {}, where f3d.md §3.1 states 252, and duration {}, as the {item} item tail)",
                    super::base_feature::EPOCH,
                    super::base_feature::DURATION
                ),
            ],
        ));
    }
    if !ctx.roster.planes.is_empty() {
        let entries = vec![
            "plane entities (display corners (0.075, 0.075) and (2.175, 2.175), empty text, flag byte 0)".to_owned(),
            format!(
                "WorkPlane scope tails, placements and placement paths (evaluation epoch {}, where f3d.md §3.1 states 252, and duration {}, as the {item} item tail)",
                super::base_feature::EPOCH,
                super::base_feature::DURATION
            ),
            "placements and frame records of the sketches on construction planes (origin at the plane entity's origin)".to_owned(),
            "record 11 (construction collection: its entities in timeline order)".to_owned(),
            "parameter source ordinals (one counter from 1 over the WorkPlane and Sketch items in timeline order: each WorkPlane item's AlongDistance parameter takes the ordinal after the parameters of the WorkPlane and Sketch items before it, the dimension parameters of the Sketch item after it continue from its ExtraOffset parameter's ordinal plus one, and an Extrude item's two parameters take the last two ordinals)".to_owned(),
        ];
        notes.extend(record_residual_unavailable(
            "F3D",
            &format!(
                "writes the WorkPlane items of the timeline whose geometry item is the {item} with these values where f3d-open-items.md DR-1004 leaves their meaning open and DR-901 leaves the rule of items before a timeline's geometry item open"
            ),
            entries,
        ));
    }
    notes.extend(record_residual_unavailable(
        "F3D",
        "stores the unit box in the Scene records of these bodies, whose vertices span no extent",
        ctx.bodies
            .iter()
            .zip(&ctx.values.boxes)
            .filter(|(_, measured)| measured.is_none())
            .map(|(body, _)| body.id.to_string()),
    ));
    notes
}
