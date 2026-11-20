// SPDX-License-Identifier: Apache-2.0
//! The record roster of a Base Feature design with result bodies (f3d.md
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
mod rows;
pub(super) mod segment;
pub(super) mod sketches;
mod units;
pub(super) mod values;
mod views;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::record_residual_unavailable;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;

use super::frames::DesignRecords;
use crate::records::references::DesignClassTag;
use crate::writer::generate::presentation::TypeRow;
use extrude::ExtrudeRecords;
use segment::TypeTable;
use sketches::SketchRoster;
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

/// The record indices of one design and the type row of each record.
pub(super) struct Roster {
    pub(super) bodies: Vec<BodyIndices>,
    pub(super) parameters: Vec<u32>,
    /// The records of the `Sketch` items, after the user parameters.
    pub(super) sketches: SketchRoster,
    /// The records of the `Extrude` item, between the `Sketch` items before
    /// and after it.
    pub(super) extrude: Option<ExtrudeRecords>,
    rows: Vec<(u32, TypeRow)>,
}

impl Roster {
    /// The one-body roster, with the ten records of each further body, one
    /// record of each user parameter, and then the records of each `Sketch`
    /// item and of `extrude`, in timeline order, at the indices from 225.
    /// The `Extrude` replaces the `Base Feature` item.
    pub(super) fn new(
        bodies: usize,
        parameters: usize,
        sketches: &[super::sketch::PlannedSketch<'_>],
        extrude: Option<&super::extrude::ExtrudePlan<'_>>,
    ) -> Result<Self, CodecError> {
        let overflow = || CodecError::NotImplemented("Base Feature record index overflow".into());
        let mut entries = rows::ONE_BODY.to_vec();
        if extrude.is_some() {
            entries.retain(|(index, _)| !BASE_FEATURE_ITEM.contains(index));
        }
        let row_of = |index: u32| {
            rows::ONE_BODY
                .iter()
                .find(|(at, _)| *at == index)
                .map(|(_, row)| *row)
                .ok_or_else(|| CodecError::malformed(format_args!("roster row {index}")))
        };
        let mut indices = vec![BodyIndices {
            design: 187,
            parameter: 215,
            design_scene: [188, 189, 190],
            parameter_scene: [220, 221, 222],
            tracking: 223,
            node: 224,
        }];
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
        let mut roster = SketchRoster::default();
        let mut extrude_records = None;
        for (position, planned) in sketches.iter().enumerate() {
            if let Some(plan) = extrude.filter(|plan| plan.after == position) {
                let (records, after) = ExtrudeRecords::allocate(plan, next, &mut entries)?;
                extrude_records = Some(records);
                next = after;
            }
            next = roster.allocate(planned, next, &mut entries)?;
        }
        if let Some(plan) = extrude.filter(|plan| plan.after == sketches.len()) {
            let (records, _) = ExtrudeRecords::allocate(plan, next, &mut entries)?;
            extrude_records = Some(records);
        }
        Ok(Self {
            bodies: indices,
            parameters: parameter_indices,
            sketches: roster,
            extrude: extrude_records,
            rows: entries,
        })
    }

    pub(super) fn rows(&self) -> &[(u32, TypeRow)] {
        &self.rows
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
    pub(super) roster: Roster,
    pub(super) values: Values,
    pub(super) bodies: Vec<RosterBody<'a>>,
    /// The `Sketch` items, in the order of [`Roster::sketches`].
    pub(super) sketches: &'a [super::sketch::PlannedSketch<'a>],
    /// The `Extrude` written in place of the `Base Feature`.
    pub(super) extrude: Option<&'a super::extrude::ExtrudePlan<'a>>,
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
        if let Some(records) = &self.roster.extrude {
            parameters.extend([records.along[1], records.taper[1]]);
        }
        parameters.sort_unstable();
        parameters
    }

    /// The largest source ordinal of the parameters the timeline items own,
    /// `0` without one: the timeline context value.
    pub(super) fn context_value(&self) -> u64 {
        let dimensions = self
            .sketches
            .iter()
            .flat_map(|sketch| &sketch.dimensions)
            .map(|dimension| dimension.parameter.ordinal);
        let extrude = self
            .extrude
            .into_iter()
            .flat_map(|plan| [plan.along.ordinal, plan.taper.ordinal]);
        dimensions.chain(extrude).max().map_or(0, u64::from)
    }

    /// The timeline item whose scope the browser node of a result body
    /// names: the `Extrude` scope, else the `Base Feature` scope.
    pub(super) fn item_scope(&self) -> u32 {
        self.roster
            .extrude
            .as_ref()
            .map_or(SCOPE, |records| records.scope)
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
    if roster.extrude.is_some() {
        known.extend(super::types::EXTRUDE_ANCESTORS);
    }
    if roster.sketches.has_dimensions() {
        known.extend(super::types::DIMENSION_ANCESTORS);
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
/// writer wrote, as the design folder's members: the segments, the history
/// stream `smbh` and the snapshot stream `smb`; and the losses of the values the
/// specification leaves open.
pub(super) fn archive(
    ctx: &Context<'_>,
    mut records: DesignRecords,
    smbh: &[u8],
    smb: &[u8],
) -> Result<(Members, Vec<LossNote>), CodecError> {
    fixed::write(ctx, &mut records)?;
    units::write(ctx, &mut records)?;
    views::write(ctx, &mut records)?;
    component::write(ctx, &mut records)?;
    bodies::write(ctx, &mut records)?;
    sketches::write(ctx, ctx.sketches, &mut records)?;
    if let Some(plan) = ctx.extrude {
        extrude::write(ctx, plan, &mut records)?;
    }
    let records = records.sorted()?;
    let design = segment::Segment {
        kind: segment::SegmentKind::Design,
        table: &ctx.table,
        records: &records,
        named: NAMED.to_vec(),
        next_entity: ctx.roster.next_entity(),
    };
    let act = act::Act::new(ctx)?;
    let browser = browser::Browser::new()?;
    let mut members = Vec::new();
    for segment in [design, act.segment(), browser.segment()] {
        members.extend(segment.members()?);
    }
    let [history, snapshot] = bodies::stream_basenames(ctx);
    members.push((
        format!("Breps.BlobParts/{history}"),
        super::history::with_header(smbh, HISTORY_FLAGS, HISTORY_SCALE)?,
    ));
    members.push((format!("Breps.BlobParts/{snapshot}"), smb.to_vec()));
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

/// The values this writer chooses where the specification leaves the rule
/// open.
fn losses(ctx: &Context<'_>) -> Vec<LossNote> {
    let mut notes = Vec::new();
    match &ctx.roster.extrude {
        None => notes.extend(super::base_feature::item_tail_loss(SCOPE)),
        Some(records) => notes.extend(extrude::losses(ctx, records)),
    }
    notes.extend(sketches::dimensions::losses(ctx));
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
    if !ctx.roster.sketches.is_empty() {
        notes.extend(record_residual_unavailable(
            "F3D",
            &format!(
                "writes the Sketch items before the {} with these records and bytes by the majority of Fusion's Sketch designs; f3d-open-items.md DR-901 leaves their rule open",
                if ctx.roster.extrude.is_some() { "Extrude" } else { "Base Feature" }
            ),
            [
                "record 199 (owner collection: the order of the Sketch and Base Feature members)".to_owned(),
                "record 200 (B65B5682: the order of its members)".to_owned(),
                "record 201 (dependency table: the order of its entries)".to_owned(),
                "record 203 (persistent-id table of Sketch items before the timeline's geometry item)".to_owned(),
                "Sketch scopes (P = 0, F = 1)".to_owned(),
                "sketch containers (word 1, sketch word 7, no base record)".to_owned(),
                "sketch points (flags and state by role)".to_owned(),
                "sketch curves (curve flag A, EntityGenesis)".to_owned(),
                "placement tracking flags (01 01 00)".to_owned(),
                format!(
                    "Sketch scope tails, placements and placement paths (evaluation epoch {}, where f3d.md §3.1 states 252, and duration {}, as the Base Feature item tail)",
                    super::base_feature::EPOCH,
                    super::base_feature::DURATION
                ),
            ],
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
