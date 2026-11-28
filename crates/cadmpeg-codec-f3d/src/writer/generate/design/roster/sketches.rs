// SPDX-License-Identifier: Apache-2.0
//! The records of the `Sketch` items a Base Feature design writes before
//! its `Base Feature` (f3d.md §3.1, "Records of a Sketch item"): each item's
//! records in Fusion's creation order after the roster, the `SketchesRoot`
//! node, and the sketch collection that lists the sketches.

pub(super) mod dimensions;

use std::collections::HashMap;

use cadmpeg_core::CodecError;

use super::super::base_feature::{timestamp_table, DURATION, EPOCH};
use super::super::frames::DesignRecords;
use super::super::sketch::{
    CurveGeometry, Entity, OriginPlane, PlannedSketch, RelationKind, Support,
};
use super::super::types;
use super::super::{Culprit, Part, RecordFailure};
use super::{count, Context, AGGREGATE};
use crate::writer::generate::presentation::TypeRow;
use cadmpeg_ir::math::portable::Portable;

/// The first persistent point and curve identity of a design's sketches.
const FIRST_IDENTITY: u64 = 100;
/// The record of type `0C5DAB90-715E-49EA-A56E-E30F2328F43E` the sketch
/// collection names.
const CURVE_COLLECTION: u32 = 22;
/// The component record (type `5D15E992-1E93-4D79-A553-10D2624148AE`).
pub(super) const COMPONENT: u32 = 205;
/// The owner collection.
pub(super) const OWNER_COLLECTION: u32 = 199;
/// The secondary identity of every written sketch curve, the null value; a
/// profile-loop entry, its side face's curve record and its sketch links
/// store the curve's (f3d.md §3.1, "Tracked results of a profile with
/// circular holes" and "Sketch links of the start cap").
pub(in crate::writer::generate::design) const CURVE_SECONDARY_IDENTITY: u64 = 0;
/// The first origin plane record, `XY`; `XZ` and `YZ` follow.
pub(super) const ORIGIN_PLANES: u32 = 29;
/// The family name of a `Sketch` scope.
pub(super) const FAMILY: &str = "Sketch";
/// The zero GUID of a scope frame.
pub(super) const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
/// The `EntityGenesis` value of a container, a point and a line.
const GENESIS: u64 = 2;
/// The `EntityGenesis` value of a circular curve.
const CIRCULAR_GENESIS: u64 = 0;
/// The sketch word of a container in the `EntityGenesis` form.
const SKETCH_WORD: u32 = 7;
/// The flags and state of the origin point.
const ORIGIN_FLAGS: [u8; 8] = [0, 0, 0, 1, 1, 1, 0, 0];
/// The flag bytes of the placement tracking flag.
pub(super) const TRACKING_FLAGS: [u8; 3] = [1, 1, 0];
/// The relation masks of f3d.md §3.1, "Sketch relations".
const COINCIDENT: u64 = 0x1;
const COLLINEAR: u64 = 0x2;
const CONCENTRIC: u64 = 0x4;
const EQUAL_LENGTH: u64 = 0x8;
const PARALLEL: u64 = 0x10;
const PERPENDICULAR: u64 = 0x20;
const HORIZONTAL: u64 = 0x40;
const VERTICAL: u64 = 0x80;
const TANGENT: u64 = 0x100;
const CURVATURE: u64 = 0x200;
const SYMMETRIC: u64 = 0x400;
const EQUAL_RADIUS: u64 = 0x800;
const MIDPOINT: u64 = 0x1000;

/// The record indices of one `Sketch` item.
#[derive(Clone, Debug)]
pub(in crate::writer::generate::design) struct SketchRecords {
    pub(in crate::writer::generate::design) container: u32,
    frame: u32,
    /// The origin point and its companion.
    origin: [u32; 2],
    /// Each point and its companion.
    points: Vec<[u32; 2]>,
    curves: Vec<u32>,
    relations: Vec<u32>,
    pub(in crate::writer::generate::design) scope: u32,
    scope_stamps: u32,
    scope_companion: u32,
    tracked: u32,
    pub(in crate::writer::generate::design) path: u32,
    path_stamps: u32,
    path_companion: u32,
    first_identity: u32,
    pub(in crate::writer::generate::design) placement: u32,
    placement_stamps: u32,
    placement_companion: u32,
    tracking_flag: u32,
    transform: u32,
    target: u32,
    second_identity: u32,
    dimensions: Vec<DimensionRecords>,
    /// The scope of the `WorkPlane` item whose plane the sketch lies on.
    pub(in crate::writer::generate::design) plane_scope: Option<u32>,
}

/// The record indices of one sketch dimension.
#[derive(Clone, Copy, Debug)]
struct DimensionRecords {
    frame: u32,
    parameter: u32,
    owner: u32,
    companion: u32,
    /// Whether it is a driven dimension.
    driven: bool,
}

impl SketchRecords {
    /// The owners of the sketch's dimension parameters, in local-ordinal
    /// order.
    pub(in crate::writer::generate::design) fn owners(&self) -> impl Iterator<Item = u32> + '_ {
        self.dimensions.iter().map(|dimension| dimension.owner)
    }

    /// The parameters of the sketch's dimensions.
    pub(in crate::writer::generate::design) fn parameters(&self) -> impl Iterator<Item = u32> + '_ {
        self.dimensions.iter().map(|dimension| dimension.parameter)
    }
}

/// The records of the design's `Sketch` items.
#[derive(Clone, Debug, Default)]
pub(in crate::writer::generate::design) struct SketchRoster {
    pub(in crate::writer::generate::design) items: Vec<SketchRecords>,
    /// The `SketchesRoot` node, where the design writes a sketch.
    root: Option<u32>,
}

impl SketchRoster {
    /// The records of `planned`, which lies on the plane of the `WorkPlane`
    /// scope `plane_scope` or on an origin plane, from record `next` in
    /// creation order, with their type rows pushed to `rows`, after the items
    /// allocated before. Returns the next free index.
    pub(super) fn allocate(
        &mut self,
        planned: &PlannedSketch<'_>,
        plane_scope: Option<u32>,
        mut next: u32,
        rows: &mut Vec<(u32, TypeRow)>,
    ) -> Result<u32, CodecError> {
        let mut take = |row: TypeRow| {
            let index = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| CodecError::NotImplemented("Sketch record index overflow".into()))?;
            rows.push((index, row));
            Ok::<_, CodecError>(index)
        };
        let container = take(types::SKETCH_CONTAINER)?;
        if self.root.is_none() {
            self.root = Some(take(types::SKETCHES_ROOT)?);
        }
        let frame = take(types::SKETCH_FRAME)?;
        let origin = [
            take(types::SKETCH_POINT)?,
            take(types::SKETCH_POINT_COMPANION)?,
        ];
        let points = planned
            .points
            .iter()
            .map(|_| {
                Ok([
                    take(types::SKETCH_POINT)?,
                    take(types::SKETCH_POINT_COMPANION)?,
                ])
            })
            .collect::<Result<Vec<_>, CodecError>>()?;
        let curves = planned
            .curves
            .iter()
            .map(|curve| {
                take(match curve.geometry {
                    CurveGeometry::Line { .. } => types::SKETCH_LINE,
                    CurveGeometry::Arc { .. } | CurveGeometry::Circle { .. } => {
                        types::SKETCH_CIRCULAR_CURVE
                    }
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let relations = planned
            .relations
            .iter()
            .map(|relation| take(relation_row(relation.kind)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut records = SketchRecords {
            container,
            frame,
            origin,
            points,
            curves,
            relations,
            scope: take(types::SKETCH_SCOPE)?,
            scope_stamps: take(types::RECORD_TIMESTAMPS)?,
            scope_companion: take(types::ITEM_TAIL_COMPANION)?,
            tracked: take(types::TRACKED_RESULTS)?,
            path: take(types::DIRECT_SELECTION)?,
            path_stamps: take(types::RECORD_TIMESTAMPS)?,
            path_companion: take(types::ITEM_TAIL_COMPANION)?,
            first_identity: take(types::TRACKED_IDENTITY)?,
            placement: take(types::SKETCH_PLACEMENT)?,
            placement_stamps: take(types::RECORD_TIMESTAMPS)?,
            placement_companion: take(types::ITEM_TAIL_COMPANION)?,
            tracking_flag: take(types::PLACEMENT_TRACKING_FLAG)?,
            transform: take(types::PLACEMENT_TRANSFORM)?,
            target: take(types::CARRIER_LINK)?,
            second_identity: take(types::TRACKED_IDENTITY)?,
            dimensions: Vec::new(),
            plane_scope,
        };
        for dimension in &planned.dimensions {
            records.dimensions.push(DimensionRecords {
                frame: take(dimensions::frame_row(dimension.kind))?,
                parameter: take(types::PARAMETER)?,
                owner: take(types::PARAMETER_OWNER)?,
                companion: take(types::RECORD_TIMESTAMPS)?,
                driven: dimension.driven,
            });
        }
        self.items.push(records);
        Ok(next)
    }

    /// Whether a `Sketch` item writes a dimension.
    pub(super) fn has_dimensions(&self) -> bool {
        self.items.iter().any(|item| !item.dimensions.is_empty())
    }

    pub(in crate::writer::generate::design) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Every record of the `Sketch` items of `ctx`, the `SketchesRoot` node and
/// the sketch collection.
pub(super) fn write(
    ctx: &Context<'_>,
    sketches: &[PlannedSketch<'_>],
    out: &mut DesignRecords,
) -> Result<(), RecordFailure> {
    let roster = &ctx.roster.sketches;
    let Some(root) = roster.root else {
        return Ok(());
    };
    #[cfg(test)]
    if let Some(error) = super::super::requested_sketch_failure() {
        return Err(error.into());
    }
    let classes = Classes::of(ctx)?;
    let mut identity = FIRST_IDENTITY;
    for (position, (ordinal, (records, planned))) in
        (1u32..).zip(roster.items.iter().zip(sketches)).enumerate()
    {
        let item = Item {
            ctx,
            classes: &classes,
            records,
            planned,
            ordinal,
            rolled_back: position >= ctx.marker,
        };
        identity = item
            .write(out, identity)
            .map_err(|failure| failure.or_named(|| Culprit::Sketch(planned.feature.clone())))?;
    }
    let containers = roster
        .items
        .iter()
        .map(|records| records.container)
        .collect::<Vec<_>>();
    let k = count(containers.len())?;
    out.record(&classes.root, root)
        .u32(0)
        .lp_ascii("SketchesRoot")?
        .zeros(2)
        .lp_ascii("SketchesRoot")?
        .u32(k);
    for container in &containers {
        out.reference(*container);
    }
    out.byte(0);
    out.record(&ctx.class_of(super::SKETCHES)?, super::SKETCHES)
        .zeros(9)
        .byte(1)
        .u32(1)
        .lp_ascii("crv_tag_id_idx")?
        .lp_ascii("IntrinsicMetaTypeuint64")?
        .u64(identity)
        .u32(k);
    for container in &containers {
        out.reference(*container);
    }
    out.u32(k)
        .byte(1)
        .byte(1)
        .reference(CURVE_COLLECTION)
        .byte(0)
        .u32(k);
    for container in &containers {
        out.reference(*container);
    }
    out.reference(AGGREGATE);
    Ok(())
}

/// The persistent identity (`crv_primary_id`) of curve `curve` of sketch
/// `sketch` of `sketches`, which [`write`] gives in item order from
/// [`FIRST_IDENTITY`]: each item's origin point, points and curves.
pub(in crate::writer::generate::design) fn curve_identity(
    sketches: &[PlannedSketch<'_>],
    sketch: usize,
    curve: usize,
) -> Result<u64, CodecError> {
    let planned = sketches
        .get(sketch)
        .ok_or_else(|| CodecError::malformed(format_args!("sketch item {sketch}")))?;
    let before = sketches[..sketch]
        .iter()
        .map(|item| 1 + item.points.len() + item.curves.len())
        .sum::<usize>();
    u64::try_from(before + 1 + planned.points.len() + curve)
        .ok()
        .and_then(|offset| FIRST_IDENTITY.checked_add(offset))
        .ok_or_else(|| CodecError::NotImplemented("sketch identity overflow".into()))
}

/// The persistent identity (`pt_tag`) of point `point` of sketch `sketch`
/// of `sketches`, which [`write`] gives after the item's origin point.
pub(super) fn point_identity(
    sketches: &[PlannedSketch<'_>],
    sketch: usize,
    point: usize,
) -> Result<u64, CodecError> {
    let before = sketches
        .get(..sketch)
        .ok_or_else(|| CodecError::malformed(format_args!("sketch item {sketch}")))?
        .iter()
        .map(|item| 1 + item.points.len() + item.curves.len())
        .sum::<usize>();
    u64::try_from(before + 1 + point)
        .ok()
        .and_then(|offset| FIRST_IDENTITY.checked_add(offset))
        .ok_or_else(|| CodecError::NotImplemented("sketch identity overflow".into()))
}

/// The relation record class: only a tangency with an endpoint branch uses the
/// tangency subclass; a tangency without one uses the shared relation class
/// with mask `0x100` and no subclass bytes.
const fn relation_row(kind: RelationKind) -> TypeRow {
    match kind {
        RelationKind::Tangent(Some(_)) => types::SKETCH_TANGENCY,
        _ => types::SKETCH_RELATION,
    }
}

/// The class tags of the records of a `Sketch` item.
pub(super) struct Classes {
    pub(super) object: crate::records::references::DesignClassTag,
    scope: crate::records::references::DesignClassTag,
    container: crate::records::references::DesignClassTag,
    member: crate::records::references::DesignClassTag,
    frame: crate::records::references::DesignClassTag,
    point: crate::records::references::DesignClassTag,
    companion: crate::records::references::DesignClassTag,
    pub(super) stamps: crate::records::references::DesignClassTag,
    pub(super) item_companion: crate::records::references::DesignClassTag,
    tracked: crate::records::references::DesignClassTag,
    path: crate::records::references::DesignClassTag,
    identity: crate::records::references::DesignClassTag,
    pub(super) placement: crate::records::references::DesignClassTag,
    pub(super) tracking_flag: crate::records::references::DesignClassTag,
    transform: crate::records::references::DesignClassTag,
    target: crate::records::references::DesignClassTag,
    root: crate::records::references::DesignClassTag,
}

impl Classes {
    pub(super) fn of(ctx: &Context<'_>) -> Result<Self, CodecError> {
        let class = |row: TypeRow| ctx.class(row.guid);
        Ok(Self {
            object: class(types::DATA_MODEL_OBJECT)?,
            scope: class(types::SKETCH_SCOPE)?,
            container: class(types::SKETCH_CONTAINER)?,
            member: class(types::SKETCH_GEOMETRY_MEMBER)?,
            frame: class(types::SKETCH_FRAME)?,
            point: class(types::SKETCH_POINT)?,
            companion: class(types::SKETCH_POINT_COMPANION)?,
            stamps: class(types::RECORD_TIMESTAMPS)?,
            item_companion: class(types::ITEM_TAIL_COMPANION)?,
            tracked: class(types::TRACKED_RESULTS)?,
            path: class(types::DIRECT_SELECTION)?,
            identity: class(types::TRACKED_IDENTITY)?,
            placement: class(types::SKETCH_PLACEMENT)?,
            tracking_flag: class(types::PLACEMENT_TRACKING_FLAG)?,
            transform: class(types::PLACEMENT_TRANSFORM)?,
            target: class(types::CARRIER_LINK)?,
            root: class(types::SKETCHES_ROOT)?,
        })
    }
}

/// Writes the records of one `Sketch` item.
struct Item<'a> {
    ctx: &'a Context<'a>,
    classes: &'a Classes,
    records: &'a SketchRecords,
    planned: &'a PlannedSketch<'a>,
    /// The item's ordinal in the `Sketch` family and in the sketch
    /// collection, from `1`.
    ordinal: u32,
    /// Whether the item stands after the timeline marker.
    rolled_back: bool,
}

impl Item<'_> {
    /// The item's records, with point and curve identities from `identity`.
    /// Returns the next free identity.
    fn write(&self, out: &mut DesignRecords, mut identity: u64) -> Result<u64, RecordFailure> {
        #[cfg(test)]
        if let Some(error) = super::super::requested_record_failure(self.planned.feature) {
            return Err(error.into());
        }
        self.container(out)?;
        self.frame(out)?;
        let mut next = || {
            let value = identity;
            identity += 1;
            value
        };
        let [record, companion] = self.records.origin;
        self.point(
            out,
            &PointRecord {
                record,
                companion,
                at: [0.0, 0.0],
                identity: next(),
                origin: true,
            },
            &[],
        )?;
        for (index, (at, [record, companion])) in self
            .planned
            .points
            .iter()
            .zip(&self.records.points)
            .enumerate()
        {
            let curves = self.curves_of(index);
            self.point(
                out,
                &PointRecord {
                    record: *record,
                    companion: *companion,
                    at: *at,
                    identity: next(),
                    origin: false,
                },
                &curves,
            )?;
        }
        for (curve, record) in self.planned.curves.iter().zip(&self.records.curves) {
            self.curve(out, curve, *record, next())?;
        }
        let ordinals = self.relations(out)?;
        self.scope(out)?;
        self.placement_path(out)?;
        self.placement(out)?;
        self.dimensions(out, ordinals)
            .map_err(|failure| failure.or_named(|| Culprit::Part(Part::Dimensions)))?;
        Ok(identity)
    }

    /// The curve records that name point `index`.
    fn curves_of(&self, index: usize) -> Vec<u32> {
        self.planned
            .curves
            .iter()
            .zip(&self.records.curves)
            .filter(|(curve, _)| curve.geometry.points().contains(&index))
            .map(|(_, record)| *record)
            .collect()
    }

    fn record_of(&self, entity: Entity) -> Result<u32, CodecError> {
        match entity {
            Entity::Point(point) => self.records.points.get(point).map(|[point, _]| *point),
            Entity::Curve(curve) => self.records.curves.get(curve).copied(),
        }
        .ok_or_else(|| CodecError::malformed(format_args!("sketch entity {entity:?}")))
    }

    /// The sketch container: the `EntityGenesis` primary record with the
    /// relation run and an empty dimension-frame run, then its Geometry
    /// member listing the origin point, the points and the curves.
    fn container(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, self.classes);
        out.record(&classes.container, records.container)
            .zeros(9)
            .byte(1)
            .u32(1);
        genesis(out, GENESIS)?;
        out.lp_utf16(&format!("0_{}", records.container))?
            .u32(u32::MAX)
            .u32(u32::MAX)
            .byte(1)
            .u32(count(records.relations.len())?);
        for relation in &records.relations {
            out.reference(*relation);
        }
        let frames = records
            .dimensions
            .iter()
            .map(|dimension| dimension.frame)
            .collect::<Vec<_>>();
        out.byte(1).u32(count(frames.len())?);
        for frame in &frames {
            out.reference(*frame);
        }
        out.byte(0)
            .u32(1)
            .u32(SKETCH_WORD)
            .byte(0)
            .byte(1)
            .u32(0)
            .byte(0)
            .byte(1)
            .reference(records.scope)
            .raw(&[0, 0, 1, 1]);
        let mut members = vec![records.origin[0]];
        members.extend(records.points.iter().map(|[point, _]| *point));
        members.extend(&records.curves);
        members.extend(frames);
        out.header(&classes.member, records.container)
            .zeros(8)
            .reference(records.frame)
            .u32(self.ordinal)
            .byte(0)
            .byte(1)
            .reference(super::SKETCHES)
            .lp_utf16("")?
            .byte(0)
            .u32(count(members.len())?);
        for member in members {
            out.reference(member);
        }
        out.reference(super::SKETCHES);
        Ok(())
    }

    /// The sketch frame record: the identity marker on `XY`, else the
    /// placement's matrix.
    fn frame(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        out.record(&self.classes.frame, self.records.frame)
            .zeros(10);
        match self.matrix()? {
            None => {
                out.byte(1).byte(0);
            }
            Some(matrix) => {
                out.byte(0);
                for value in matrix {
                    out.f64(value);
                }
                out.byte(0);
            }
        }
        out.reference(AGGREGATE);
        Ok(())
    }

    /// The construction plane the sketch lies on.
    fn plane(
        &self,
        index: usize,
    ) -> Result<
        (
            &super::super::sketch::PlannedPlane<'_>,
            &super::planes::PlaneRecords,
        ),
        CodecError,
    > {
        self.ctx
            .planes
            .get(index)
            .zip(self.ctx.roster.planes.get(index))
            .ok_or_else(|| CodecError::malformed(format_args!("construction plane {index}")))
    }

    /// The row-major matrix the placement and the frame record store: the
    /// fixed matrix of an origin plane, `None` for the identity of `XY`, or
    /// the frame of a construction plane.
    fn matrix(&self) -> Result<Option<[f64; 16]>, CodecError> {
        Ok(match self.planned.support {
            Support::Origin(plane) => matrix(plane),
            Support::Construction(index) => Some(self.plane(index)?.0.matrix()),
        })
    }

    /// The Design entity ID of the plane the sketch lies on.
    fn entity(&self) -> Result<u64, CodecError> {
        Ok(match self.planned.support {
            Support::Origin(plane) => u64::from(ORIGIN_PLANES + plane.ordinal()),
            Support::Construction(index) => u64::from(self.plane(index)?.1.entity),
        })
    }

    /// The origin and the two axes the placement transform stores.
    fn transform(&self) -> Result<[f64; 9], CodecError> {
        Ok(match self.planned.support {
            Support::Origin(plane) => transform(plane),
            Support::Construction(index) => {
                let plane = self.plane(index)?.0;
                let ([u, v, _], o) = (plane.axes(), plane.origin());
                [o[0], o[1], o[2], u[0], u[1], u[2], v[0], v[1], v[2]]
            }
        })
    }

    /// A point in the `EntityGenesis` form with its companion, which lists
    /// `curves`. The origin point stores Fusion's origin flags and state `0`;
    /// the other points store zero flags and state `1`.
    fn point(
        &self,
        out: &mut DesignRecords,
        point: &PointRecord,
        curves: &[u32],
    ) -> Result<(), CodecError> {
        let classes = self.classes;
        let &PointRecord {
            record,
            companion,
            at,
            identity,
            origin,
        } = point;
        out.record(&classes.point, record).zeros(9).byte(1).u32(2);
        genesis(out, GENESIS)?;
        property(out, "pt_tag", identity)?;
        out.reference(companion)
            .raw(&if origin { ORIGIN_FLAGS } else { [0; 8] })
            .f64(at[0])
            .f64(at[1])
            .f64(0.0)
            .u64(0)
            .byte(u8::from(!origin))
            .zeros(12)
            .f32(1.0)
            .f32(1.0)
            .raw(&[0, 1, 0, 0, 0])
            .reference(companion)
            .reference(self.records.container);
        out.record(&classes.companion, companion)
            .zeros(10)
            .u32(count(curves.len())?);
        for curve in curves {
            out.reference(*curve);
        }
        out.byte(0).reference(record);
        Ok(())
    }

    /// A line or circular curve in the `EntityGenesis` form: the analytic
    /// payload in cm on the sketch plane, its point references, and the
    /// common curve tail.
    fn curve(
        &self,
        out: &mut DesignRecords,
        curve: &super::super::sketch::PlannedCurve,
        record: u32,
        identity: u64,
    ) -> Result<(), CodecError> {
        let (row, genesis_value) = match curve.geometry {
            CurveGeometry::Line { .. } => (types::SKETCH_LINE, GENESIS),
            CurveGeometry::Arc { .. } | CurveGeometry::Circle { .. } => {
                (types::SKETCH_CIRCULAR_CURVE, CIRCULAR_GENESIS)
            }
        };
        let payload = curve_payload(&curve.geometry, &self.planned.points)?;
        // A class is looked up where the design writes a record of its type:
        // the type table lists the types of the written records.
        out.record(&self.ctx.class(row.guid)?, record)
            .zeros(9)
            .byte(1)
            .u32(3);
        genesis(out, genesis_value)?;
        property(out, "crv_primary_id", identity)?;
        property(out, "crv_secondary_id", CURVE_SECONDARY_IDENTITY)?;
        for value in payload {
            out.f64(value);
        }
        let references = curve.geometry.points();
        for point in &references {
            out.reference(self.record_of(Entity::Point(*point))?);
        }
        let curve_flag = u8::from(!self.planned.relations.is_empty());
        let pad = if references.len() == 1 { 5 } else { 3 };
        out.zeros(pad)
            .byte(curve_flag)
            .byte(0)
            .zeros(2)
            .u32(u32::from(curve.construction))
            .byte(1)
            .zeros(4)
            .byte(1)
            .zeros(12)
            .f32(1.0)
            .f32(1.0)
            .zeros(5)
            .reference(self.records.container);
        Ok(())
    }

    /// The relations: members in ascending record index with their relation
    /// ordinals, the container, the mask, and the members in semantic order.
    /// Returns the number of relation ordinals each member has taken.
    fn relations(&self, out: &mut DesignRecords) -> Result<HashMap<u32, u32>, CodecError> {
        let mut named = HashMap::<u32, u32>::new();
        for (relation, record) in self.planned.relations.iter().zip(&self.records.relations) {
            let semantic = relation
                .members
                .iter()
                .map(|member| self.record_of(*member))
                .collect::<Result<Vec<_>, _>>()?;
            let mut ascending = semantic.clone();
            ascending.sort_unstable();
            ascending.dedup();
            let tangent = match relation.kind {
                RelationKind::Tangent(Some(ends)) => Some(ends.bytes()),
                _ => None,
            };
            let row = relation_row(relation.kind);
            out.record(&self.ctx.class(row.guid)?, *record)
                .zeros(8)
                .byte(1)
                .u32(count(ascending.len())?);
            for member in &ascending {
                let ordinal = named.entry(*member).or_insert(0);
                out.reference(*member).u32(*ordinal);
                *ordinal += 1;
            }
            out.byte(0);
            for flag in tangent.into_iter().flatten() {
                out.byte(flag);
            }
            out.reference(self.records.container)
                .u64(mask(relation.kind))
                .u32(count(semantic.len())?);
            for member in &semantic {
                out.reference(*member);
            }
            out.byte(0);
        }
        Ok(named)
    }

    /// The `Sketch` scope with its 78-byte item tail, the scope's
    /// record-timestamp table, item-tail companion and tracked-results
    /// table.
    fn scope(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, self.classes);
        let stamp = self.ctx.values.stamps.scope;
        let owners = records.owners().collect::<Vec<_>>();
        out.record(&classes.scope, records.scope)
            .zeros(9)
            .byte(0)
            .byte(0)
            .u32(count(owners.len())?);
        for owner in &owners {
            out.reference(*owner);
        }
        out.byte(1)
            .u32(0)
            .reference(records.container)
            .reference(records.placement)
            .u32(0)
            .u32(0)
            .u32(0)
            .lp_utf16(NULL_GUID)?
            .zeros(3)
            .u32(count(2 + owners.len())?)
            .reference(records.path)
            .reference(records.placement);
        for owner in &owners {
            out.reference(*owner);
        }
        out.u32(u32::MAX).lp_utf16(FAMILY)?;
        item_tail(
            out,
            classes,
            &ItemTail {
                scope: records.scope,
                ordinal: self.ordinal,
                tracked: records.tracked,
                companion: records.scope_companion,
                stamps: records.scope_stamps,
                rolled_back: self.rolled_back,
            },
        );
        let mut entries = vec![
            (COMPONENT, self.ctx.values.stamps.component),
            (records.scope, stamp),
            (records.placement, stamp),
        ];
        entries.extend(owners.iter().map(|owner| (*owner, stamp)));
        timestamp_table(
            out,
            &classes.stamps,
            records.scope_stamps,
            true,
            &entries,
            (records.scope, stamp),
        )?;
        item_companion(out, &classes.item_companion, records.scope_companion);
        tracked_results(out, classes, records.tracked);
        Ok(())
    }

    /// The placement path and its records.
    fn path_records(&self) -> PathRecords {
        let records = self.records;
        PathRecords {
            word: SKETCH_PATH_WORD,
            scope: records.scope,
            path: records.path,
            stamps: records.path_stamps,
            companion: records.path_companion,
            first_identity: records.first_identity,
            transform: records.transform,
            target: records.target,
            second_identity: records.second_identity,
        }
    }

    /// The placement path, its record-timestamp table and item-tail
    /// companion, and the first tracked identity of the sketch's plane.
    fn placement_path(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        placement_path(
            self.ctx,
            self.classes,
            out,
            &self.path_records(),
            self.entity()?,
        )
    }

    /// The placement on the origin plane, its record-timestamp table,
    /// item-tail companion and tracking flag, the placement transform, its
    /// target and the second tracked identity.
    fn placement(&self, out: &mut DesignRecords) -> Result<(), CodecError> {
        let (records, classes) = (self.records, self.classes);
        let stamp = self.ctx.values.stamps.scope;
        out.record(&classes.placement, records.placement)
            .zeros(10)
            .byte(0)
            .f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .byte(0)
            .byte(1)
            .zeros(6)
            .zeros(3);
        match self.matrix()? {
            None => {
                out.byte(1);
            }
            Some(matrix) => {
                out.byte(0);
                for value in matrix {
                    out.f64(value);
                }
            }
        }
        out.f64(0.0)
            .f64(0.0)
            .f64(0.0)
            .u32(0)
            .byte(0)
            .u32(0)
            .byte(0)
            .byte(0)
            .u32(2)
            .byte(0)
            .u32(0);
        placement_base(
            out,
            classes,
            &PlacementBase {
                placement: records.placement,
                path: records.path,
                tracking_flag: records.tracking_flag,
                companion: records.placement_companion,
                stamps: records.placement_stamps,
                scope: records.scope,
            },
        );
        timestamp_table(
            out,
            &classes.stamps,
            records.placement_stamps,
            true,
            &[(records.path, stamp)],
            (records.placement, stamp),
        )?;
        item_companion(out, &classes.item_companion, records.placement_companion);
        out.record(&classes.tracking_flag, records.tracking_flag)
            .zeros(10)
            .raw(&TRACKING_FLAGS);
        placement_transform(
            classes,
            out,
            &self.path_records(),
            self.transform()?,
            self.entity()?,
        );
        Ok(())
    }
}

/// The u32 at offset 219 of a `Sketch` item's placement path.
const SKETCH_PATH_WORD: u32 = 4;

/// The records of a placement path: the path, its record-timestamp table and
/// item-tail companion, the two tracked entity identities, the placement
/// transform and its target, and the scope of the item that owns them.
pub(super) struct PathRecords {
    /// The u32 at offset 219 of the path.
    pub(super) word: u32,
    pub(super) scope: u32,
    pub(super) path: u32,
    pub(super) stamps: u32,
    pub(super) companion: u32,
    pub(super) first_identity: u32,
    pub(super) transform: u32,
    pub(super) target: u32,
    pub(super) second_identity: u32,
}

/// The placement path of `records`, its record-timestamp table and
/// item-tail companion, and its first tracked identity naming `entity`.
pub(super) fn placement_path(
    ctx: &Context<'_>,
    classes: &Classes,
    out: &mut DesignRecords,
    records: &PathRecords,
    entity: u64,
) -> Result<(), CodecError> {
    let stamp = ctx.values.stamps.scope;
    out.record(&classes.path, records.path)
        .zeros(10)
        .reference(records.first_identity)
        .u32(1)
        .lp_utf16(crate::manifest::GENERATED_ASSET_FOLDER_GUID)?
        .lp_utf16(&ctx.values.lower_guid("component-a"))?
        .u32(2)
        .u32(0)
        .u32(1)
        .reference(records.transform)
        .u32(0)
        .u32(0x21)
        .u32(records.word)
        .zeros(6)
        .u32(EPOCH)
        .f64(DURATION)
        .u32(EPOCH)
        .reference(records.companion)
        .raw(&[1, 0, 0])
        .reference(records.stamps)
        .byte(0)
        .reference(records.scope)
        .header(&classes.object, records.path)
        .zeros(8)
        .reference(OWNER_COLLECTION);
    timestamp_table(
        out,
        &classes.stamps,
        records.stamps,
        true,
        &[(records.path, stamp)],
        (records.path, stamp),
    )?;
    item_companion(out, &classes.item_companion, records.companion);
    identity(classes, out, records.first_identity, entity);
    Ok(())
}

/// The placement transform of `records` with the origin and two axes
/// `triples`, its target, and the second tracked identity naming `entity`.
pub(super) fn placement_transform(
    classes: &Classes,
    out: &mut DesignRecords,
    records: &PathRecords,
    triples: [f64; 9],
    entity: u64,
) {
    out.record(&classes.transform, records.transform)
        .zeros(10)
        .byte(0);
    for value in triples {
        out.f64(value);
    }
    out.byte(1).reference(records.target);
    out.record(&classes.target, records.target)
        .zeros(10)
        .reference(records.second_identity)
        .byte(0);
    identity(classes, out, records.second_identity, entity);
}

/// A tracked entity identity naming the Design entity `entity`.
fn identity(classes: &Classes, out: &mut DesignRecords, record: u32, entity: u64) {
    out.record(&classes.identity, record).zeros(10).u64(entity);
}

/// The records a placement's base level names.
pub(super) struct PlacementBase {
    pub(super) placement: u32,
    pub(super) path: u32,
    pub(super) tracking_flag: u32,
    pub(super) companion: u32,
    pub(super) stamps: u32,
    pub(super) scope: u32,
}

/// The base level of a placement after its `values` list, its paired
/// header and the owner collection reference.
pub(super) fn placement_base(out: &mut DesignRecords, classes: &Classes, base: &PlacementBase) {
    out.u32(1)
        .reference(base.path)
        .zeros(2)
        .u32(1)
        .reference(base.tracking_flag)
        .u32(0)
        .u32(4)
        .zeros(10)
        .u32(EPOCH)
        .f64(DURATION)
        .u32(EPOCH)
        .reference(base.companion)
        .raw(&[1, 0, 0])
        .reference(base.stamps)
        .byte(0)
        .reference(base.scope)
        .header(&classes.object, base.placement)
        .zeros(8)
        .reference(OWNER_COLLECTION);
}

/// The records a scope's 78-byte feature item tail names.
pub(super) struct ItemTail {
    pub(super) scope: u32,
    /// The family ordinal of the item, from `1`.
    pub(super) ordinal: u32,
    pub(super) tracked: u32,
    pub(super) companion: u32,
    pub(super) stamps: u32,
    /// Whether the item stands after the timeline marker: its rolled-back
    /// flag `s1`.
    pub(super) rolled_back: bool,
}

/// The 78-byte feature item tail of an unsuppressed item with no label and
/// no history state, the paired header and the owner collection reference.
/// An item after the timeline marker sets both `s0` and `s1` (f3d.md §3.1,
/// "In every form, the state bytes").
pub(super) fn item_tail(out: &mut DesignRecords, classes: &Classes, tail: &ItemTail) {
    out.u32(tail.ordinal)
        .u32(0)
        .u32(0)
        .u32(0)
        .zeros(3)
        .reference(tail.tracked)
        .byte(0)
        .u32(u32::MAX)
        .u32(EPOCH)
        .f64(DURATION)
        .u32(EPOCH)
        .reference(tail.companion)
        .byte(1)
        .byte(u8::from(tail.rolled_back))
        .byte(u8::from(tail.rolled_back))
        .reference(tail.stamps)
        .zeros(2)
        .header(&classes.object, tail.scope)
        .zeros(8)
        .reference(OWNER_COLLECTION);
}

/// The 53-byte tracked-results table with no entry and next key `1`.
pub(super) fn tracked_results(out: &mut DesignRecords, classes: &Classes, record: u32) {
    out.record(&classes.tracked, record)
        .zeros(10)
        .u32(0)
        .u64(1)
        .header(&classes.object, record)
        .zeros(9);
}

/// The 96-byte analytic payload of a line or circular curve in cm on the
/// sketch plane, with its points from `points`.
fn curve_payload(geometry: &CurveGeometry, points: &[[f64; 2]]) -> Result<[f64; 12], CodecError> {
    let at = |index: usize| {
        points
            .get(index)
            .copied()
            .ok_or_else(|| CodecError::malformed(format_args!("sketch point {index}")))
    };
    Ok(match *geometry {
        CurveGeometry::Line { start, end } => {
            let ([x0, y0], [x1, y1]) = (at(start)?, at(end)?);
            let (dx, dy) = (x1 - x0, y1 - y0);
            let length = dx.portable_hypot(dy);
            let (ux, uy) = if length > 0.0 {
                (dx / length, dy / length)
            } else {
                (1.0, 0.0)
            };
            [x0, y0, 0.0, dx, dy, 0.0, ux, uy, 0.0, 0.0, 0.0, 1.0]
        }
        CurveGeometry::Arc {
            center,
            radius,
            angles,
            ..
        } => {
            let [cx, cy] = at(center)?;
            [
                cx, cy, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, radius, angles[0], angles[1],
            ]
        }
        CurveGeometry::Circle { center, radius } => {
            let [cx, cy] = at(center)?;
            [
                cx,
                cy,
                0.0,
                0.0,
                0.0,
                1.0,
                1.0,
                0.0,
                0.0,
                radius,
                0.0,
                std::f64::consts::TAU,
            ]
        }
    })
}

/// One point record and its companion.
struct PointRecord {
    record: u32,
    companion: u32,
    /// The point in sketch coordinates in cm.
    at: [f64; 2],
    identity: u64,
    origin: bool,
}

/// The 33-byte item-tail companion with item word `0`.
pub(super) fn item_companion(
    out: &mut DesignRecords,
    class: &crate::records::references::DesignClassTag,
    record: u32,
) {
    out.record(class, record).zeros(9).byte(1).zeros(8).u32(0);
}

/// The `EntityGenesis` property of type `IntrinsicMetaTypeuint64`.
pub(super) fn genesis(out: &mut DesignRecords, value: u64) -> Result<(), CodecError> {
    property(out, "EntityGenesis", value)
}

/// A property of type `IntrinsicMetaTypeuint64`: its LP-ASCII name and type
/// and the u64 value.
fn property(out: &mut DesignRecords, name: &str, value: u64) -> Result<(), CodecError> {
    out.lp_ascii(name)?
        .lp_ascii("IntrinsicMetaTypeuint64")?
        .u64(value);
    Ok(())
}

fn mask(kind: RelationKind) -> u64 {
    match kind {
        RelationKind::Coincident => COINCIDENT,
        RelationKind::Collinear => COLLINEAR,
        RelationKind::Concentric => CONCENTRIC,
        RelationKind::EqualLength => EQUAL_LENGTH,
        RelationKind::EqualRadius => EQUAL_RADIUS,
        RelationKind::Parallel => PARALLEL,
        RelationKind::Perpendicular => PERPENDICULAR,
        RelationKind::Horizontal => HORIZONTAL,
        RelationKind::Vertical => VERTICAL,
        RelationKind::Tangent(_) => TANGENT,
        RelationKind::Curvature => CURVATURE,
        RelationKind::Symmetric => SYMMETRIC,
        RelationKind::Midpoint => MIDPOINT,
    }
}

/// The row-major matrix a placement on `plane` stores; `None` for the
/// identity of `XY`.
pub(super) fn matrix(plane: OriginPlane) -> Option<[f64; 16]> {
    match plane {
        OriginPlane::Xy => None,
        OriginPlane::Xz => Some([
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]),
        OriginPlane::Yz => Some([
            0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]),
    }
}

/// The origin and the two axes the placement transform of `plane` stores.
pub(super) fn transform(plane: OriginPlane) -> [f64; 9] {
    let [o, u, v] = plane.entity_frame();
    [o[0], o[1], o[2], u[0], u[1], u[2], v[0], v[1], v[2]]
}
