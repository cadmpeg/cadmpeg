// SPDX-License-Identifier: Apache-2.0
//! The `Contents/Config-0` stream of a generated part: its one `moPart_c`
//! object (`docs/formats/sldprt.md` §2.3 "Configuration objects").

use cadmpeg_core::CodecError;

use super::archive::{Archive, ClassMap};
use super::block::{block, block_close, block_open, named, named_with, Oid, Schema, Tail, TAIL};
use super::detailing::{Detail, DETAILING_14000};
use super::tree::{unit_component, PartRefs};
use super::{ids, Body, Facts, FILETIME, PLANE_HALF_EXTENT, TIME_STAMP, VERSION};

/// The u32 after the part's identifier in its named head.
const PART_HEAD_WORD: u32 = 0x100;

/// The first object identifier the part's bodies and atoms take.
pub(super) const FIRST_BODY_ID: u32 = 101;

/// Density in kg/m³ and modelling tolerance in metres.
const DENSITY: f64 = 1000.0;
const TOLERANCE: f64 = 1.0e-8;

/// The colours a `moTransRefPlaneData_c` writes (`0x00BBGGRR`).
const PLANE_COLOUR: u32 = 0x00f7_8553;
const PLANE_EDGE_COLOUR: u32 = 0x0067_2305;

/// The marker block's opening (§2.3).
const MARKER_OPENING: [u8; 8] = [0xaa, 0xaa, 0xaa, 0xaa, 0xbb, 0xbb, 0xbb, 0xbb];

/// The record list's count base and flag word (§2.3).
const RECORD_BASE: u32 = 10_000;
const RECORD_FLAGS: u32 = 0x2000_0000;
const RECORD_TYPE: u32 = 20;

/// The u32 pair after the views (§2.3).
const VIEW_LIST_END: [u32; 2] = [50, 108];

/// The byte runs `sldprt.md` gives as lengths only (EV-19), written as zero
/// bytes: the bytes between the sketch-block manager and the exploded-view
/// manager.
const THREAD_DATA_RUN: usize = 300;

/// An atom's 64 bytes after its named head.
const ATOM_RUN: usize = 64;

/// The view index the first annotation view takes; the notes-area view
/// follows the annotation views.
const FIRST_ANNOTATION_VIEW: u32 = 200;

/// The scale a view states at +58 of its bytes: `1`, or `0.8` in the
/// notes-area view.
const VIEW_SCALE: f64 = 1.0;
const NOTES_AREA_SCALE: f64 = 0.8;

/// The `gcCurvatureObject_c` values.
const CURVATURE: [f64; 5] = [1000.0, 31.6, 4.0, 1.2, 1.0];

/// The standard views every part lists: name and, for all but the first, the
/// orientation rows.
const VIEWS: [(&str, Option<[f64; 9]>); 10] = [
    ("*Normal To", None),
    ("*Front", None),
    (
        "*Back",
        Some([-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -1.0]),
    ),
    (
        "*Left",
        Some([0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]),
    ),
    (
        "*Right",
        Some([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0]),
    ),
    ("*Top", Some([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0])),
    (
        "*Bottom",
        Some([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0]),
    ),
    (
        "*Isometric",
        Some([
            0.707_106_781_186_546_2,
            -0.408_204_055_911_355_17,
            0.577_381_545_199_983_9,
            0.0,
            0.816_540_811_885_749_9,
            0.577_287_712_085_542_6,
            -0.707_106_781_186_548_8,
            -0.408_204_055_911_353_7,
            0.577_381_545_199_981_7,
        ]),
    ),
    (
        "*Trimetric",
        Some([
            0.884_418_121_677_421_4,
            -0.240_365_894_683_921_36,
            0.400_036_026_779_313_94,
            0.0,
            0.857_167_300_702_134,
            0.515_038_074_910_018_1,
            -0.466_695_388_930_062_1,
            -0.455_509_006_804_273_25,
            0.758_094_294_050_286_8,
        ]),
    ),
    (
        "*Dimetric",
        Some([
            0.935_414_346_724_141_1,
            -0.117_851_130_142_681_38,
            0.333_333_333_266_778_1,
            0.0,
            0.942_809_041_610_104,
            0.333_333_333_254_022_4,
            -0.353_553_390_512_166_05,
            -0.311_804_782_167_191_8,
            0.881_917_103_743_329_1,
        ]),
    ),
];

/// Writes the stream and returns it with the class map it ends with and the
/// indices the tree refers to.
pub(super) fn config(facts: &Facts<'_>) -> Result<(Vec<u8>, ClassMap, PartRefs), CodecError> {
    let mut a = Archive::new(facts.id_base);
    a.object("moPart_c");
    let first_section = a.len();
    a.u32(0);
    a.u32(0);
    a.u32(VERSION);
    named_with(
        &mut a,
        Some(facts.name),
        Schema::Plain,
        Oid::Plain(0),
        PART_HEAD_WORD,
    );
    a.u32(0);
    a.object("moVisualProperties_c");
    visual(&mut a);
    units(&mut a);
    a.object("moUnitComponent_c");
    a.u16(2);
    a.u8(0);
    a.u32(0);
    a.u32(0);
    a.u8(0);
    a.object("gcCurvatureObject_c");
    a.f64s(&CURVATURE);
    a.u32(FILETIME.1);
    a.u32(FILETIME.0);
    transformed_planes(&mut a);
    a.zeros(16);
    a.f64(DENSITY);
    a.f64(TOLERANCE);
    a.u32(2);
    a.u16(0);
    let next_id = facts
        .bodies
        .last()
        .map_or(Some(FIRST_BODY_ID), |body| body.id.checked_add(1))
        .ok_or_else(|| {
            CodecError::malformed("SLDPRT native skeleton: the body identifiers exceed their field")
        })?;
    a.bytes(&MARKER_OPENING);
    a.u32(0);
    a.u32(1);
    a.zeros(78);
    a.u32(next_id);
    a.u16(0);
    a.object("gcXhatch_c");
    a.cstring("ANSI31 (Iron BrickStone)");
    a.f64s(&[0.0, 1.0, 0.0]);
    a.u32s(&[1, u32::MAX, u32::MAX, 0]);
    material(&mut a, facts.bodies.len());
    let mut bodies = Vec::new();
    let mut document = None;
    for body in facts.bodies {
        bodies.push(body_record(&mut a, facts, body, &mut document));
    }
    a.zeros(20);
    let second_section = a.len();
    a.u32(0);
    a.u32(0);
    a.u16(0);
    a.u32(next_id);
    a.u32(1);
    a.object("moAtom_c");
    atom_head(&mut a);
    a.zeros(ATOM_RUN);
    a.u32(VERSION);
    a.u32(RECORD_BASE + 1);
    a.u32(RECORD_FLAGS | 1);
    a.u32s(&[RECORD_BASE + 1, RECORD_TYPE, 0]);
    a.i32(-1);
    a.u32(1);
    a.i32(-1);
    a.zeros(8);
    relations(&mut a);
    let environment = environment(&mut a);
    a.null();
    a.object("suObList");
    a.u16(0);
    a.zeros(4);
    for (index, (name, rows)) in (0..).zip(VIEWS) {
        a.object("moView_c");
        view(&mut a, name, rows, index, VIEW_SCALE);
        a.u16(0);
    }
    a.u8(0);
    a.u32s(&VIEW_LIST_END);
    a.object_with_schema("moFeatColorTab_c", 0);
    a.u16(0);
    a.u32s(&[0, 0, 0, 0]);
    block_manager(&mut a);
    a.zeros(THREAD_DATA_RUN);
    a.object("moPrtExplViewManager_c");
    named(&mut a, None, Schema::Plain, Oid::Tree(ids::EXPLODED_VIEWS));
    block(&mut a, Tail { word64: 0, ..TAIL });
    a.zeros(8);
    a.bytes(&[0xff; 8]);
    a.zeros(26);
    a.u32(FIRST_BODY_ID);
    a.zeros(8);
    let (mut bytes, map) = a.finish()?;
    patch_section_lengths(&mut bytes, first_section, second_section)?;
    Ok((
        bytes,
        map,
        PartRefs {
            environment,
            bodies,
        },
    ))
}

/// An atom's named head: no name, u32 `1`, no identifier.
fn atom_head(a: &mut Archive) {
    a.null();
    a.u32(1);
    a.u16(0);
    a.u8(0);
    a.u8(0x40);
    a.u32(u32::MAX);
    a.u32(0);
    a.cstring("");
}

/// The part's first u32 is the byte count from its end to the u32 after the
/// twenty zero bytes; that u32 is the byte count from its end to the end of
/// the stream.
fn patch_section_lengths(bytes: &mut [u8], first: usize, second: usize) -> Result<(), CodecError> {
    let length = |from: usize, to: usize| {
        from.checked_add(4)
            .and_then(|start| to.checked_sub(start))
            .and_then(|length| u32::try_from(length).ok())
            .ok_or_else(|| {
                CodecError::malformed(
                    "SLDPRT native skeleton: a moPart_c section length does not fit its field",
                )
            })
    };
    let first_length = length(first, second)?;
    let second_length = length(second, bytes.len())?;
    for (at, value) in [(first, first_length), (second, second_length)] {
        if let Some(slot) = bytes.get_mut(at..at + 4) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

/// A `moVisualProperties_c` body (§2.3) of a white part with no appearance.
fn visual(a: &mut Archive) {
    a.u32(0x00ff_ffff);
    a.u32(0);
    a.u32(0x00c0_c0c0);
    a.cstring("");
    a.cstring("");
    a.f64s(&[1.0, 1.0, 1.0, 0.3125, 0.0, 0.3]);
    a.u32(0);
    a.u32s(&[1, 0, 1, 0, 1]);
    a.cstring("");
    a.u32(1);
    a.u32(0);
    a.cstring("");
    a.u32(0);
    a.cstring("");
    a.u32(0);
    a.u32(1);
    a.cstring("");
    a.zeros(20);
}

/// One unit of the units table: the four-word base form, the block-container
/// fields, and the bytes `class` adds (§2.3).
struct Unit {
    class: &'static str,
    kind: u32,
    value: f64,
    decimals: u32,
    words: [u32; 4],
}

/// The base words of most units.
const UNIT_WORDS: [u32; 4] = [1, 0, 1, 1];

/// The step value most units state.
const UNIT_STEP: f64 = 0.01;

/// The angle a unit states after its decimal places: the little-endian f64
/// of π/4 rounded to fourteen decimal places.
const UNIT_ANGLE: [u8; 8] = [0x28, 0x2d, 0x44, 0x54, 0xfb, 0x21, 0xe9, 0x3f];

const UNITS: [Unit; 15] = [
    Unit {
        class: "moAngleUserUnits_c",
        kind: 3,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moNumberUserUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moDensityUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moFloatNumberUserUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moSpringConstantUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moUnitSysUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moForceUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moStressUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moGravityUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moLinearMotorUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moRotaryMotorUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moPowerUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moEnergyUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moTimeUnits_c",
        kind: 3,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
    Unit {
        class: "moFrequencyUserUnits_c",
        kind: 2,
        value: UNIT_STEP,
        decimals: 10,
        words: UNIT_WORDS,
    },
];

/// The length unit (millimetres) and its dual unit (inches).
const LENGTH: Unit = Unit {
    class: "moLengthUserUnits_c",
    kind: 3,
    value: 0.1,
    decimals: 4,
    words: [0, 0, 0, 1],
};
const DUAL_LENGTH: Unit = Unit {
    class: "moLengthUserUnits_c",
    kind: 2,
    value: 1.0,
    decimals: 1,
    words: [0, 0, 0, 1],
};

/// The display unit codes a `moLengthUserUnits_c` states (§2.3).
const MILLIMETRE: u16 = 0;
const INCH: u16 = 3;

/// The u16 values a unit class adds.
fn unit_word(class: &str) -> Option<u16> {
    match class {
        "moUnitSysUnits_c" => Some(4),
        "moForceUnits_c" => Some(2),
        "moAngleUserUnits_c" | "moPowerUnits_c" | "moEnergyUnits_c" | "moTimeUnits_c" => Some(0),
        _ => None,
    }
}

/// A unit's base form and block-container fields.
fn unit_base(a: &mut Archive, unit: &Unit) {
    a.u32(unit.kind);
    a.u32(1);
    a.f64(unit.value);
    a.u32(unit.decimals);
    a.u32(1);
    a.bytes(&UNIT_ANGLE);
    a.u32s(&unit.words);
    a.f64(0.0);
    a.u32(0);
    a.u16(0);
}

/// A length unit in millimetres with its inch dual unit written in place.
fn length_unit(a: &mut Archive) {
    a.object(LENGTH.class);
    unit_base(a, &LENGTH);
    a.u16(MILLIMETRE);
    a.object(DUAL_LENGTH.class);
    unit_base(a, &DUAL_LENGTH);
    a.u16(INCH);
    a.null();
}

/// The `moUnitsTable_c` of sixteen units (§2.3).
fn units(a: &mut Archive) {
    a.object("moUnitsTable_c");
    a.u16(16);
    length_unit(a);
    for unit in &UNITS {
        a.object(unit.class);
        unit_base(a, unit);
        if let Some(word) = unit_word(unit.class) {
            a.u16(word);
        }
        if unit.class == "moDensityUnits_c" {
            a.u16(3);
            a.u16(1);
            a.u8(1);
            length_unit(a);
            a.u16(3);
            a.u32(0);
        }
    }
}

/// `moTransRefPlaneData_c`: u32 `1`, `1`, two colours, `0x5f`, `0`, the box
/// flag, the box, u32 `0` and no entries.
fn transformed_planes(a: &mut Archive) {
    a.object("moTransRefPlaneData_c");
    a.u32s(&[1, 1, PLANE_COLOUR, PLANE_EDGE_COLOUR, 0x5f, 0]);
    a.u8(1);
    let h = PLANE_HALF_EXTENT;
    a.f64s(&[0.0, 0.0, 0.0, h, h, h, -h, -h, -h, 0.0]);
    a.u32(0);
    a.u32(0);
}

/// `moMaterial_c`: its density parameter, model data, the body count, u32
/// and, with bodies, a visual-properties body.
fn material(a: &mut Archive, bodies: usize) {
    a.object("moMaterial_c");
    a.object("moDensityParameter_c");
    named_with(a, None, Schema::Plain, Oid::Plain(u32::MAX), 0);
    a.u16(0);
    a.f64(DENSITY);
    a.u8(0);
    a.object("uoModelData_c");
    a.zeros(6);
    let count = a.count32(bodies, "body count");
    a.u32(count);
    a.u32(u32::from(bodies != 0));
    if bodies != 0 {
        visual(a);
    }
}

/// One body record of the part: a `moCompSolidBody_c` whose face reference
/// names a face of the body by its base-body feature, the 28 bytes
/// (u32 `1`, the body identifier, i32 `-1`, u32 `0`, u32 `0`, u32 `1`, the
/// body identifier) and a clear visual-properties flag. Returns the
/// `moCompSolidBody_c`'s map index.
fn body_record(a: &mut Archive, facts: &Facts<'_>, body: &Body, document: &mut Option<u32>) -> u32 {
    let index = a.object("moCompSolidBody_c");
    unit_component(a, body.id);
    a.object("moFaceRef_c");
    a.u32s(&[1, 0, 1, 0, 0]);
    a.bytes(&FACE_REF_MARKER);
    a.u16(0);
    a.object("moSimpleSurfIdRep_c");
    a.u16(0);
    a.object("moFR_c");
    document_object(a, facts, document);
    a.u32(body.feature_id);
    a.u32(TIME_STAMP);
    a.u32(1);
    a.zeros(8);
    a.u32(VERSION);
    a.u32(0);
    a.u32(0);
    a.u32s(&[1, body.id]);
    a.i32(-1);
    a.u32s(&[0, 0, 1, body.id]);
    a.u32(0);
    index
}

/// The 16 bytes every face and edge reference header holds (§2.3).
const FACE_REF_MARKER: [u8; 16] = [
    0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54, 0x7d, 0xc3, 0x94, 0x25, 0xad, 0x49, 0xb2, 0x54,
];

/// The part's own document as a `moExtObject_c` (§2.3): its path and name,
/// u16 `2`, a byte, a u32, three empty `CString` values, eighteen bytes,
/// the configuration name and twelve bytes. A second use is a reference.
fn document_object(a: &mut Archive, facts: &Facts<'_>, document: &mut Option<u32>) {
    if let Some(index) = *document {
        a.reference(index);
        return;
    }
    *document = Some(a.object("moExtObject_c"));
    a.object("moCStringHandle_c");
    a.cstring(&format!("{}.SLDPRT", facts.name));
    a.object("moCStringHandle_c");
    a.cstring(facts.name);
    a.u16(2);
    a.u8(0);
    a.u32(TIME_STAMP);
    a.cstring("");
    a.cstring("");
    a.cstring("");
    a.zeros(18);
    a.cstring(facts.active);
    a.zeros(12);
}

/// `moRelMgr_c`: no relations, u16 `0` and the detailing settings.
fn relations(a: &mut Archive) {
    a.object("moRelMgr_c");
    a.u16(0);
    a.u16(0);
    for detail in DETAILING_14000 {
        match *detail {
            Detail::Run(length) => a.zeros(length),
            Detail::Text(text) => a.cstring(text),
            Detail::LengthUnit => {
                a.object(LENGTH.class);
            }
        }
    }
}

/// One light of the environment folder: the feature and its own fields
/// (§2.3).
struct Light {
    class: &'static str,
    name: &'static str,
    id: u32,
    colour: [f64; 3],
    flags: [u32; 5],
    switched: u32,
    direction: Option<[f64; 3]>,
}

const LIGHTS: [Light; 3] = [
    Light {
        class: "moAmbientLight_c",
        name: "Ambient",
        id: ids::AMBIENT,
        colour: [0.3, 0.0, 0.0],
        flags: [0x00ff_ffff, 0, 1, 0, 1],
        switched: 1,
        direction: None,
    },
    Light {
        class: "moDirectionLight_c",
        name: "Directional1",
        id: ids::DIRECTIONAL_1,
        colour: [0.0, 0.4, 0.4],
        flags: [0x00ff_ffff, 0, 1, 0, 1],
        switched: 0,
        direction: Some([
            0.305_119_921_134_965_8,
            0.457_066_229_439_054_6,
            0.058_204_737_856_618_27,
        ]),
    },
    Light {
        class: "moDirectionLight_c",
        name: "Directional2",
        id: ids::DIRECTIONAL_2,
        colour: [0.02, 0.5, 0.4],
        flags: [0x00ff_ffff, 0, 0, 0, 1],
        switched: 0,
        direction: Some([
            0.064_246_119_145_839_22,
            0.265_806_899_412_846,
            0.480_223_514_912_824_6,
        ]),
    },
];

/// The light's name string, after its own colour fields.
const LIGHT_NAME: &str = "UnNamed";
const AMBIENT_NAME: &str = "Ambient-1";

/// The `moEnvFolder_c` with its lights as inputs. Returns its map index.
fn environment(a: &mut Archive) -> u32 {
    let index = a.object("moEnvFolder_c");
    named(
        a,
        Some("Lights, Cameras and Scene"),
        Schema::Plain,
        Oid::Tree(ids::LIGHTS),
    );
    let lights = a.count16(LIGHTS.len(), "light count");
    block_open(a, lights);
    for light in &LIGHTS {
        a.object(light.class);
        named(a, Some(light.name), Schema::Geometric, Oid::Tree(light.id));
        block(a, Tail { word64: 0, ..TAIL });
        a.f64s(&light.colour);
        a.u32s(&light.flags);
        a.cstring(if light.direction.is_some() {
            LIGHT_NAME
        } else {
            AMBIENT_NAME
        });
        a.u32s(&[0, 0, 0]);
        a.f64(1.0);
        a.u32(light.switched);
        a.f64(16.0);
        a.u32(0);
        if let Some(direction) = light.direction {
            a.f64s(&direction);
            a.zeros(8);
        }
    }
    block_close(a, TAIL);
    a.u32(0);
    index
}

/// A view's name, frame flag, frame rows and its 115 bytes (§2.3): f64 `1`
/// at +24, the scale at +58, i32 `-1` at +69, f64 `-1` at +73 and the view
/// index at +85, the other bytes zero.
fn view(a: &mut Archive, name: &str, rows: Option<[f64; 9]>, index: u32, scale: f64) {
    a.cstring(name);
    match rows {
        Some(rows) => {
            a.u8(1);
            a.f64s(&rows);
        }
        None => a.u8(0),
    }
    a.zeros(24);
    a.f64(1.0);
    a.zeros(26);
    a.f64(scale);
    a.zeros(3);
    a.i32(-1);
    a.f64(-1.0);
    a.zeros(4);
    a.u32(index);
    a.zeros(26);
}

/// A `moAnnotationView_c`: the view and its 50 bytes (§2.3): u32 `1` at
/// +28, i32 `-1` at +34, u32 `1` at +38, and i32 `-1` at +42 and +46.
fn annotation_view(a: &mut Archive, name: &str, rows: Option<[f64; 9]>, index: u32, scale: f64) {
    a.object("moAnnotationView_c");
    view(a, name, rows, index, scale);
    a.zeros(28);
    a.u32(1);
    a.zeros(2);
    a.i32(-1);
    a.u32(1);
    a.i32(-1);
    a.i32(-1);
}

/// `moSketchBlockMgr_c` (§2.3): a named head (null name) and block, twelve
/// bytes, u32, identifier, u32, u16, a list of the two annotation views,
/// u32, u32, one identifier, 30 bytes, its `moPMarkRecord_c`, a list of the
/// notes-area view, u32 and two `CString` values.
fn block_manager(a: &mut Archive) {
    a.object("moSketchBlockMgr_c");
    named_with(a, None, Schema::Plain, Oid::Plain(u32::MAX), 0);
    block(a, TAIL);
    a.zeros(12);
    a.u32(0);
    a.u32(BLOCK_MANAGER_ID);
    a.u32(0);
    a.u16(0);
    a.object("suObList");
    a.u16(2);
    annotation_view(a, "*Front", None, FIRST_ANNOTATION_VIEW, VIEW_SCALE);
    annotation_view(a, "*Top", VIEWS[5].1, FIRST_ANNOTATION_VIEW + 1, VIEW_SCALE);
    a.u32(0);
    a.u32(BLOCK_MANAGER_WORD);
    a.u32(1);
    a.u32(0);
    a.zeros(30);
    a.object("moPMarkRecord_c");
    a.u32(MARK_KIND);
    a.u32(1);
    a.u32(0);
    a.u32(0);
    a.u16(0);
    a.zeros(30);
    a.object("suObList");
    a.u16(1);
    annotation_view(
        a,
        "Notes Area",
        None,
        FIRST_ANNOTATION_VIEW + 2,
        NOTES_AREA_SCALE,
    );
    a.u32(0);
    a.cstring("");
    a.cstring("");
}

/// The sketch-block manager's identifier.
const BLOCK_MANAGER_ID: u32 = 100;

/// The sketch-block manager's `moPMarkRecord_c` *X* (§2.3), which writes a
/// further u32 `1`.
const MARK_KIND: u32 = 4;

/// The u32 the sketch-block manager writes before its identifier count.
const BLOCK_MANAGER_WORD: u32 = 199;
