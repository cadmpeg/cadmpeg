// SPDX-License-Identifier: Apache-2.0
//! The `Contents/Config-0-ResolvedFeatures` stream of a generated part: the
//! default feature tree and one `moBaseBody_c` stored body per body
//! (`docs/formats/sldprt.md` §2.2, §2.3).

use cadmpeg_core::CodecError;

use super::archive::{Archive, ClassMap};
use super::block::{
    block, block_close, block_open, body_head, named, Oid, Schema, Tail, ADDED_FOLDER_TAIL, TAIL,
};
use super::{ids, Body, Facts, Feature, PLANE_HALF_EXTENT, TIME_STAMP};

/// The sketch-record attribute block of the origin point: `ff ff`, a u16,
/// a byte, eight `ff` bytes and f32 `-1`.
const ORIGIN_ATTRIBUTE: [u8; 17] = [
    0xff, 0xff, 0x1f, 0x00, 0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x80,
    0xbf,
];

/// The relation type of the origin point's fixed relation.
const FIXED_RELATION: u32 = 17;

/// The u32 a sketch tail opens its group list with: `100000` and no groups.
const NO_CONTOUR_GROUPS: u32 = 100_000;

/// The block tail of a stored body: created in the part's version, kind `3`
/// and +60 `0`.
const BASE_BODY_TAIL: Tail = Tail {
    kind: 3,
    word30: 0,
    word60: 0,
    created: true,
    word64: 0,
    ..TAIL
};

/// The top-level objects of the default tree: twelve folders, three planes
/// and their plane data, and the origin.
const DEFAULT_TREE_OBJECTS: usize = 19;

/// The value of the reference-plane field at block end +136.
const PLANE_WORD: u32 = 5;

/// Writes the stream: the map count `Config-0` ended with, a u16 (the
/// top-level object count minus two) and the top-level objects.
pub(super) fn resolved_features(
    facts: &Facts<'_>,
    config: ClassMap,
    part: &PartRefs,
) -> Result<Vec<u8>, CodecError> {
    let count = config.count();
    let mut archive = Archive::continuing(config, facts.id_base);
    let comments = Tail { word64: 0, ..TAIL };
    plain_folder(
        &mut archive,
        "moCommentsFolder_c",
        "Comments",
        Oid::Tree(ids::COMMENTS),
        comments,
    );
    favorites(&mut archive);
    history(&mut archive);
    selection_sets(&mut archive);
    plain_folder(
        &mut archive,
        "moSensorFolder_c",
        "Sensors",
        Oid::Tree(ids::SENSORS),
        TAIL,
    );
    plain_folder(
        &mut archive,
        "moDocsFolder_c",
        "Design Binder",
        Oid::Tree(ids::DESIGN_BINDER),
        TAIL,
    );
    annotations(&mut archive);
    plain_folder(
        &mut archive,
        "moInkMarkupFolder_c",
        "Markups",
        Oid::Tree(ids::MARKUPS),
        ADDED_FOLDER_TAIL,
    );
    plain_folder(
        &mut archive,
        "moSurfaceBodyFolder_c",
        "Surface Bodies",
        Oid::Tree(ids::SURFACE_BODIES),
        TAIL,
    );
    archive.zeros(12);
    plain_folder(
        &mut archive,
        "moSolidBodyFolder_c",
        "Solid Bodies",
        Oid::Tree(ids::SOLID_BODIES),
        TAIL,
    );
    archive.zeros(12);
    archive.reference(part.environment);
    plain_folder(
        &mut archive,
        "moEqnFolder_c",
        "Equations",
        Oid::Tree(ids::EQUATIONS),
        TAIL,
    );
    plain_folder(
        &mut archive,
        "moMaterialFolder_c",
        super::MATERIAL_NAME,
        Oid::Tree(ids::MATERIAL),
        TAIL,
    );
    archive.cstring("");
    archive.u32(0);
    for plane in Plane::ALL {
        reference_plane(&mut archive, plane);
        plane_data(&mut archive, plane);
    }
    origin(&mut archive);
    // Dimension parameters take identifiers after the stored bodies' and the
    // user features'.
    let features = archive.count32(facts.bodies.len() + facts.features.len(), "feature count");
    let first = archive.tree_id(ids::MARKUPS + 1);
    let mut next_id = match first.checked_add(features) {
        Some(id) => id,
        None => archive.count32(usize::MAX, "object identifier"),
    };
    let mut profiles = Vec::new();
    for feature in facts.features {
        let profile = match feature {
            Feature::Sketch(sketch) => Some(super::sketch::profile_feature(
                &mut archive,
                sketch,
                &mut next_id,
            )?),
            Feature::Extrusion(extrusion) => {
                let profile = profiles.get(extrusion.profile).copied().flatten();
                let Some(profile) = profile else {
                    return Err(CodecError::malformed(
                        "SLDPRT native skeleton: an extrusion's profile is no earlier sketch",
                    ));
                };
                let parameter = super::sketch::take_id(&mut archive, &mut next_id);
                super::extrude::extrusion(&mut archive, extrusion, profile, part.atom, parameter);
                None
            }
        };
        profiles.push(profile);
    }
    for (body, &component) in facts.bodies.iter().zip(&part.bodies) {
        base_body(&mut archive, body, component);
    }
    // The u16 after the map count is the top-level object count minus two.
    let tops = archive.count16(
        DEFAULT_TREE_OBJECTS - 2 + facts.features.len() + facts.bodies.len(),
        "top-level object count",
    );
    let (objects, _) = archive.finish()?;
    let mut stream = count.to_le_bytes().to_vec();
    stream.extend_from_slice(&tops.to_le_bytes());
    stream.extend_from_slice(&objects);
    Ok(stream)
}

/// What the tree refers to in `Config-0`.
pub(super) struct PartRefs {
    /// The map index of the `moEnvFolder_c` object.
    pub(super) environment: u32,
    /// The map index of the `moAtom_c` object.
    pub(super) atom: u32,
    /// The map index of each body's `moCompSolidBody_c` or
    /// `moCompSurfaceBody_c` object.
    pub(super) bodies: Vec<u32>,
}

/// A folder of `class` named `name`: its named head, its block and the
/// folder's u32 `0`.
fn plain_folder(archive: &mut Archive, class: &'static str, name: &str, id: Oid, tail: Tail) {
    archive.object(class);
    named(archive, Some(name), Schema::Plain, id);
    block(archive, tail);
    archive.u32(0);
}

/// `moFavoriteFolder_c`: the folder and u32 `0`, u32 `1`.
fn favorites(archive: &mut Archive) {
    plain_folder(
        archive,
        "moFavoriteFolder_c",
        "Favorites",
        Oid::Tree(ids::FAVORITES),
        TAIL,
    );
    archive.u32(0);
    archive.u32(1);
}

/// `moHistoryFolder_c` with no items.
fn history(archive: &mut Archive) {
    plain_folder(
        archive,
        "moHistoryFolder_c",
        "History",
        Oid::Tree(ids::HISTORY),
        TAIL,
    );
    archive.u16(0);
}

/// `moSelectionSetFolder_c`: the folder, u16 `0` and u32 `1`.
fn selection_sets(archive: &mut Archive) {
    plain_folder(
        archive,
        "moSelectionSetFolder_c",
        "Selection Sets",
        Oid::Tree(ids::SELECTION_SETS),
        ADDED_FOLDER_TAIL,
    );
    archive.u16(0);
    archive.u32(1);
}

/// The `moDetailCabinet_c` annotations folder. Its inputs are the notes
/// folder pair: the opening folder, whose link writes the end-tag folder in
/// place, the run `1 1 1 0 1 1` and a reference to the opening folder; the
/// end-tag folder's link is a reference back to the opening folder.
fn annotations(archive: &mut Archive) {
    archive.object("moDetailCabinet_c");
    named(
        archive,
        Some("Annotations"),
        Schema::Plain,
        Oid::Tree(ids::ANNOTATIONS),
    );
    block_open(archive, 2);
    let notes = archive.object("moNotesAreaFtrFolder_c");
    named(archive, Some("Notes"), Schema::Plain, Oid::Tree(ids::NOTES));
    block(archive, ADDED_FOLDER_TAIL);
    archive.u32(0);
    archive.object("moNotesAreaFtrFolder_c");
    named(
        archive,
        Some("Notes1___EndTag___"),
        Schema::Plain,
        Oid::Tree(ids::NOTES_END),
    );
    block(archive, ADDED_FOLDER_TAIL);
    archive.u32(0);
    archive.reference(notes);
    archive.zeros(8);
    archive.u32s(&[1, 1, 1, 0, 1, 1]);
    archive.reference(notes);
    block_close(archive, Tail { word64: 0, ..TAIL });
    archive.u16(0);
    archive.f64(1.0);
    archive.f64(1.0);
    archive.zeros(10);
}

/// A default reference plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::writer) enum Plane {
    Front,
    Top,
    Right,
}

impl Plane {
    const ALL: [Self; 3] = [Self::Front, Self::Top, Self::Right];

    fn name(self) -> &'static str {
        match self {
            Self::Front => "Front Plane",
            Self::Top => "Top Plane",
            Self::Right => "Right Plane",
        }
    }

    fn id(self) -> Oid {
        Oid::Tree(self.tree_id())
    }

    /// The plane's `ids` value.
    pub(super) fn tree_id(self) -> u32 {
        match self {
            Self::Front => ids::FRONT_PLANE,
            Self::Top => ids::TOP_PLANE,
            Self::Right => ids::RIGHT_PLANE,
        }
    }

    /// The plane's first and second in-plane axes and its normal in model
    /// space: the columns of its rotation.
    pub(in crate::writer) fn axes(self) -> [[f64; 3]; 3] {
        match self {
            Self::Front => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            Self::Top => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            Self::Right => [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
        }
    }

    /// The plane's rotation rows, for the planes that state one.
    fn rotation(self) -> Option<[f64; 9]> {
        match self {
            Self::Front => None,
            Self::Top => Some([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0]),
            Self::Right => Some([-0.0, 0.0, 1.0, -0.0, 1.0, 0.0, -1.0, 0.0, 0.0]),
        }
    }

    /// The plane's normal: Front is the XY plane, Top the XZ plane and Right
    /// the YZ plane.
    fn normal(self) -> [f64; 3] {
        self.axes()[2]
    }
}

/// `moRefPlane_c`: the feature, its body head and the plane's own bytes
/// (§2.3).
fn reference_plane(archive: &mut Archive, plane: Plane) {
    archive.object("moRefPlane_c");
    named(archive, Some(plane.name()), Schema::Geometric, plane.id());
    block(archive, Tail { word64: 0, ..TAIL });
    body_head(archive, None, &[]);
    archive.zeros(6);
    archive.u32(0);
    archive.zeros(44);
    archive.f64s(&[-PLANE_HALF_EXTENT, PLANE_HALF_EXTENT, 0.0]);
    archive.u32(PLANE_WORD);
    archive.zeros(10);
}

/// `moDefaultRefPlnData_c`: the plane's point and normal, its rotation, its
/// display half-extents and centre (§2.3).
fn plane_data(archive: &mut Archive, plane: Plane) {
    archive.object("moDefaultRefPlnData_c");
    archive.f64s(&[0.0, 0.0, 0.0]);
    archive.f64s(&plane.normal());
    match plane.rotation() {
        Some(rows) => {
            archive.u8(1);
            archive.f64s(&rows);
        }
        None => archive.u8(0),
    }
    archive.f64s(&[0.0, 0.0, 0.0, 1.0]);
    archive.u8(0);
    archive.f64s(&[
        PLANE_HALF_EXTENT,
        -PLANE_HALF_EXTENT,
        PLANE_HALF_EXTENT,
        -PLANE_HALF_EXTENT,
    ]);
    archive.zeros(6);
    archive.i32(-4);
    archive.i32(-4);
    archive.u8(0);
    archive.f64s(&[0.0, 0.0, 0.0]);
    archive.u32(1);
    archive.zeros(12);
}

/// `moOriginProfileFeature_c`: the feature, ten zero bytes and its `sgSketch`
/// of one point with a fixed relation, ending with its plane, the Front
/// plane, as a `moCompRefPlane_c`.
fn origin(archive: &mut Archive) {
    archive.object("moOriginProfileFeature_c");
    named(
        archive,
        Some("Origin"),
        Schema::Geometric,
        Oid::Tree(ids::ORIGIN),
    );
    block(archive, Tail { word64: 0, ..TAIL });
    archive.zeros(10);
    archive.object("sgSketch");
    archive.u32(1);
    archive.u16(0);
    archive.u16(1);
    archive.u16(0);
    archive.u32(1);
    origin_point(archive);
    archive.u32(0);
    archive.zeros(36);
    archive.zeros(6);
    archive.u16(1);
    archive.zeros(8);
    archive.u32(u32::MAX);
    archive.zeros(16);
    archive.u32(FIXED_RELATION);
    archive.u32(2);
    archive.u16(0);
    archive.u32(0xfffe);
    archive.object("sgPointHandle");
    archive.u16(0);
    archive.u32(u32::MAX);
    archive.u32(0);
    archive.zeros(14);
    archive.object("suObList");
    archive.u16(0);
    archive.zeros(4);
    archive.u16(0);
    archive.u16(0);
    archive.zeros(4);
    archive.zeros(76);
    archive.cstring("");
    archive.zeros(42);
    archive.u32(1);
    archive.u32(0);
    archive.u32(0);
    archive.zeros(24);
    archive.null();
    archive.zeros(38);
    archive.u32(3);
    archive.u8(1);
    archive.u32(0);
    archive.u16(0);
    archive.u32(0);
    archive.u32(NO_CONTOUR_GROUPS);
    archive.u16(0);
    archive.null();
    origin_plane(archive);
}

/// The origin point's sketch record (§2 point records).
fn origin_point(archive: &mut Archive) {
    archive.bytes(&ORIGIN_ATTRIBUTE);
    archive.u32(1);
    archive.u16(0);
    archive.u16(4);
    archive.u16(2);
    archive.u16(1);
    archive.u16(0);
    archive.f32(-1.0);
    archive.null();
    archive.u16(4);
    archive.zeros(9);
    archive.f64(1.0);
    archive.zeros(8);
    archive.u16(0x1f);
    archive.f64(0.0);
    archive.f64(0.0);
    archive.u16(1);
    archive.u16(0);
    archive.null();
    archive.null();
    archive.null();
    archive.u32(0xffff_fffe);
    archive.zeros(42);
    archive.u32(0);
}

/// The origin sketch's plane: a `moCompRefPlane_c` naming the Front plane by
/// its identifier, with the identity frame and an empty trailer.
fn origin_plane(archive: &mut Archive) {
    archive.object("moCompRefPlane_c");
    unit_component(archive, 0);
    let front = archive.tree_id(ids::FRONT_PLANE);
    archive.u32(front);
    archive.u32(TIME_STAMP);
    archive.u16(0);
    archive.u16(3);
    archive.u16(0);
    archive.u8(0);
    archive.f64s(&[0.0, 0.0, 0.0, 1.0]);
    archive.zeros(3);
    archive.u32(3);
    archive.zeros(12);
    archive.u32(0);
    archive.u16(0);
    archive.u16(0);
    archive.zeros(4);
}

/// A `moUnitComponent_c` in its long form (§2.3): u16 `2`, the flag word,
/// 39 bytes (the body identifier twice at +3 and +7 when `body` is not
/// zero), sixteen `ff` bytes and twenty zero bytes.
pub(super) fn unit_component(archive: &mut Archive, body: u32) {
    archive.object("moUnitComponent_c");
    archive.u16(2);
    archive.u16(0);
    archive.zeros(3);
    archive.u32(body);
    archive.u32(body);
    archive.zeros(28);
    archive.bytes(&[0xff; 16]);
    archive.zeros(20);
}

/// `moBaseBody_c`: the feature, a body head whose input is the body's
/// record in `Config-0` and whose identifier list holds the body identifier,
/// a `moPMarkRecord_c` naming the body by identifier and GUID, an empty
/// `CString`, u32 `1` and the feature name. The next object follows the name
/// directly (§2.3).
fn base_body(archive: &mut Archive, body: &Body, component: u32) {
    archive.object("moBaseBody_c");
    named(
        archive,
        Some(&body.name),
        Schema::Plain,
        Oid::Plain(body.feature_id),
    );
    block(archive, BASE_BODY_TAIL);
    body_head(archive, Some(std::slice::from_ref(&component)), &[body.id]);
    archive.object("moPMarkRecord_c");
    archive.u32(10);
    archive.u32(0);
    archive.u32(0);
    archive.zeros(8);
    archive.u32s(&[0, 0, 0, 0]);
    archive.u32(0);
    archive.u32(0);
    archive.u32(0);
    archive.u32(1);
    archive.u32(body.id);
    archive.cstring(&body.guid);
    archive.zeros(14);
    archive.cstring("");
    archive.u32(1);
    archive.cstring(&body.name);
}
