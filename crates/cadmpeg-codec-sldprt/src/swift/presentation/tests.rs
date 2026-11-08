use super::faces::tests::{declared_reference, tagged_reference};
use super::{annotations, parse, Archive, FeaturePath};
use crate::brep::feature_source::FeatureSourceId;
use crate::brep::PersistentFaceIdentity;
use crate::swift::{project, Entity, Reference, ROOT_CLASS};
use cadmpeg_ir::ids::{FaceId, PmiId};
use cadmpeg_ir::pmi::zones::ToleranceConstraints;
use cadmpeg_ir::pmi::{
    GeometricToleranceKind, PmiAnnotation, PmiDefinition, PmiMagnitude, PmiQuantity, PmiTarget,
};
use cadmpeg_ir::scalar::NonNegativeReal;

const FEATURE_REFERENCE_TAG: u16 = 0x8040;
const MIRROR_TAG: u16 = 0x8050;
const SCHEMA_VERSION: u32 = 11000;
const NO_ID: u32 = u32::MAX;

fn declaration(class: &str) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xff, 0x01, 0x00];
    bytes.extend_from_slice(
        &u16::try_from(class.len())
            .expect("class length")
            .to_le_bytes(),
    );
    bytes.extend_from_slice(class.as_bytes());
    bytes
}

fn framed(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut bytes = vec![
        0xff,
        0xfe,
        0xff,
        u8::try_from(units.len()).expect("short text"),
    ];
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// A name object (`moNodeName_c` tag and string) followed by the named-object
/// header with its object ID at +8.
fn name(text: &str, id: u32) -> Vec<u8> {
    let mut bytes = 0x8003u16.to_le_bytes().to_vec();
    bytes.extend(framed(text));
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0, 0, 0, 0x40]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes
}

enum FeatureReference {
    Declared,
    Tagged,
}

fn path(features: &[(FeatureReference, u32, u32)]) -> Vec<u8> {
    let mut bytes = u16::try_from(features.len())
        .expect("path length")
        .to_le_bytes()
        .to_vec();
    for (form, id, stamp) in features {
        match form {
            FeatureReference::Declared => bytes.extend(declaration("moSwiftFR_c")),
            FeatureReference::Tagged => {
                bytes.extend_from_slice(&FEATURE_REFERENCE_TAG.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&stamp.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes
}

struct Gtol<'a> {
    name: &'a str,
    paths: Vec<Vec<u8>>,
    rows: &'a [(&'a str, &'a str, &'a [&'a str])],
    note: &'a str,
}

fn gtol(first: bool, display: &Gtol<'_>) -> Vec<u8> {
    let mut bytes = if first {
        declaration("moDisplayGtol_c")
    } else {
        0x8008u16.to_le_bytes().to_vec()
    };
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&6u32.to_le_bytes());
    if first {
        bytes.extend(declaration("moCompGtol_c"));
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend(declaration("moGtolData_c"));
    } else {
        bytes.extend_from_slice(&0x800au16.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&0x800cu16.to_le_bytes());
    }
    bytes.extend(name(display.name, NO_ID));
    for path in &display.paths {
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend_from_slice(path);
    }
    for (symbol, tolerance, labels) in display.rows {
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend(framed(symbol));
        bytes.extend(framed(tolerance));
        for label in *labels {
            bytes.extend_from_slice(&[0; 4]);
            bytes.extend(framed(label));
        }
    }
    bytes.extend_from_slice(&[0; 8]);
    for text in ["", "", "<Gtol>", display.note] {
        bytes.extend(framed(text));
    }
    bytes.extend_from_slice(&[0; 18]);
    bytes
}

struct Image<'a> {
    count: Option<u16>,
    gtols: Vec<Gtol<'a>>,
    datum_tags: bool,
    mirrors: &'a [(&'a str, u32)],
}

impl Image<'_> {
    fn bytes(&self) -> Vec<u8> {
        let mut bytes = SCHEMA_VERSION.to_le_bytes().to_vec();
        bytes.extend(declaration("moSwiftSchema_c"));
        bytes.extend(declaration("moNodeName_c"));
        bytes.extend(framed("Scheme2"));
        bytes.extend_from_slice(&[0; 12]);
        bytes.extend(declaration("moDetailFolder_c"));
        bytes.extend(name("Detail Folder2", NO_ID));
        bytes.extend_from_slice(&[0; 10]);
        let count = self
            .count
            .unwrap_or_else(|| u16::try_from(self.gtols.len()).expect("display count"));
        bytes.extend_from_slice(&count.to_le_bytes());
        for (index, display) in self.gtols.iter().enumerate() {
            bytes.extend(gtol(index == 0, display));
        }
        if self.datum_tags {
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend(declaration("moDisplayDatumTag_c"));
            bytes.extend_from_slice(&[0; 8]);
        }
        for (mirror, id) in self.mirrors {
            bytes.extend_from_slice(&MIRROR_TAG.to_le_bytes());
            bytes.extend(name(mirror, *id));
            bytes.extend_from_slice(&[0; 8]);
        }
        bytes
    }
}

fn reference(id: &str, class: &str) -> Reference {
    Reference {
        id: id.into(),
        class: format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net"),
    }
}

fn entity(class: &str) -> Entity {
    Entity {
        class: format!("PrizMetrik.GdtAnalysis.{class}"),
        ..Entity::default()
    }
}

fn feature(name: &str, attribute: i32) -> Entity {
    let mut feature = entity("GdtPlane");
    feature.strings.insert("ObjectName".into(), name.into());
    feature.integers.insert("UserAttribute".into(), attribute);
    feature
}

fn flatness(feature: &str, tolerance: f64) -> Entity {
    let mut annotation = entity("GdtFlatness");
    annotation.doubles.insert("Tolerance".into(), tolerance);
    annotation
        .features
        .references
        .push(reference(feature, "GdtPlane"));
    annotation
}

/// Two planes (`UserAttribute` 100 and `ObjectName` "Plane B"), two
/// flatness frames on the first plane and one on the second.
fn root() -> Entity {
    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    root.features.references = vec![reference("F1", "GdtPlane"), reference("F2", "GdtPlane")];
    root.features.entities = vec![feature("Plane A", 100), feature("Plane B", -101)];
    root.annotations.references = vec![
        reference("A1", "GdtFlatness"),
        reference("A2", "GdtFlatness"),
        reference("A3", "GdtFlatness"),
    ];
    root.annotations.entities = vec![
        flatness("F1", 0.381),
        flatness("F1", 0.127),
        flatness("F2", 0.254),
    ];
    root
}

const MIRRORS: &[(&str, u32)] = &[("Plane A", 100), ("Plane B", 200)];

fn two_row_display<'a>(name: &'a str, id: u32, note: &'a str, first: bool) -> Gtol<'a> {
    let form = if first {
        FeatureReference::Declared
    } else {
        FeatureReference::Tagged
    };
    Gtol {
        name,
        paths: vec![path(&[(form, id, 7)])],
        rows: &[("<GTOL-FLAT>", ".015", &[]), ("<GTOL-FLAT>", ".005", &[])],
        note,
    }
}

fn presentations(image: &Image<'_>) -> (Vec<PmiAnnotation>, usize) {
    let root = root();
    let mut projected = project(&root);
    let archive = parse(&image.bytes());
    annotations(&archive, &root, &mut projected, &[]).expect("presentation annotations")
}

fn only<T>(items: &[T]) -> &T {
    let [item] = items else { panic!("one item") };
    item
}

fn depicted(annotation: &PmiAnnotation) -> (Option<&str>, Vec<&str>) {
    let PmiDefinition::Presentation {
        text, semantics, ..
    } = &annotation.definition
    else {
        panic!("presentation definition")
    };
    (
        text.as_deref(),
        semantics
            .iter()
            .map(cadmpeg_ir::ids::PmiId::as_str)
            .collect(),
    )
}

#[test]
fn a_display_depicts_the_tolerances_of_features_selected_by_attribute_or_name() {
    let image = Image {
        count: None,
        gtols: vec![
            two_row_display("DetailItem1", 100, "SIM REQT 1", true),
            Gtol {
                name: "DetailItem2",
                paths: vec![path(&[(FeatureReference::Tagged, 200, 9)])],
                rows: &[("<GTOL-FLAT>", ".010", &[])],
                note: "",
            },
        ],
        datum_tags: true,
        mirrors: MIRRORS,
    };
    let (presentations, unresolved) = presentations(&image);
    assert_eq!(unresolved, 0);
    let [first, second] = presentations.as_slice() else {
        panic!("two presentations")
    };
    assert_eq!(first.name.as_deref(), Some("DetailItem1"));
    assert_eq!(
        depicted(first),
        (
            Some("SIM REQT 1"),
            vec!["sldprt:model:pmi#A1", "sldprt:model:pmi#A2"]
        )
    );
    assert_eq!(depicted(second), (None, vec!["sldprt:model:pmi#A3"]));
}

#[test]
fn a_feature_path_selects_its_outer_feature_and_admits_member_suffixes() {
    let image = Image {
        count: None,
        gtols: vec![Gtol {
            name: "DetailItem1",
            paths: vec![
                path(&[(FeatureReference::Declared, 200, 9)]),
                path(&[
                    (FeatureReference::Tagged, 300, 4),
                    (FeatureReference::Tagged, 200, 9),
                ]),
            ],
            rows: &[("<GTOL-FLAT>", ".010", &[])],
            note: "",
        }],
        datum_tags: true,
        mirrors: &[("Plane A", 100), ("Plane B", 200), ("Pattern", 300)],
    };
    let archive = parse(&image.bytes());
    assert_eq!(
        only(&archive.gtols).path,
        FeaturePath::Features(vec![300, 200])
    );
    let root = root();
    let (presentations, unresolved) =
        annotations(&archive, &root, &mut project(&root), &[]).expect("presentation annotations");
    assert_eq!(unresolved, 0);
    assert_eq!(
        depicted(only(&presentations)).1,
        vec!["sldprt:model:pmi#A3"]
    );
}

#[test]
fn row_kinds_that_differ_from_the_feature_tolerances_leave_the_display_unlinked() {
    let image = Image {
        count: None,
        gtols: vec![Gtol {
            name: "DetailItem1",
            paths: vec![path(&[(FeatureReference::Declared, 100, 7)])],
            rows: &[("<GTOL-FLAT>", ".015", &[])],
            note: "2 SURFACES",
        }],
        datum_tags: true,
        mirrors: MIRRORS,
    };
    let (presentations, unresolved) = presentations(&image);
    assert_eq!(unresolved, 1);
    assert_eq!(depicted(only(&presentations)), (Some("2 SURFACES"), vec![]));
}

#[test]
fn feature_paths_that_disagree_leave_the_display_unlinked() {
    let image = Image {
        count: None,
        gtols: vec![Gtol {
            name: "DetailItem1",
            paths: vec![
                path(&[(FeatureReference::Declared, 100, 7)]),
                path(&[(FeatureReference::Tagged, 200, 9)]),
            ],
            rows: &[("<GTOL-FLAT>", ".010", &[])],
            note: "",
        }],
        datum_tags: true,
        mirrors: MIRRORS,
    };
    let archive = parse(&image.bytes());
    assert_eq!(only(&archive.gtols).path, FeaturePath::Inconsistent);
    let root = root();
    let (_, unresolved) =
        annotations(&archive, &root, &mut project(&root), &[]).expect("presentation annotations");
    assert_eq!(unresolved, 1);
}

#[test]
fn a_display_list_count_that_disagrees_with_its_items_yields_no_displays() {
    let image = Image {
        count: Some(3),
        gtols: vec![
            two_row_display("DetailItem1", 100, "", true),
            two_row_display("DetailItem2", 100, "", false),
        ],
        datum_tags: true,
        mirrors: MIRRORS,
    };
    let archive: Archive = parse(&image.bytes());
    assert!(archive.gtols.is_empty());
    assert!(archive.unbounded.is_empty());
}

#[test]
fn the_last_display_without_a_following_list_is_unbounded() {
    let image = Image {
        count: None,
        gtols: vec![
            two_row_display("DetailItem1", 100, "", true),
            two_row_display("DetailItem2", 100, "", false),
        ],
        datum_tags: false,
        mirrors: MIRRORS,
    };
    let archive = parse(&image.bytes());
    assert_eq!(archive.gtols.len(), 1);
    assert_eq!(archive.unbounded.len(), 1);
    let root = root();
    let (presentations, unresolved) =
        annotations(&archive, &root, &mut project(&root), &[]).expect("presentation annotations");
    assert_eq!(presentations.len(), 1);
    assert_eq!(unresolved, 1);
}

fn pmi(id: &str) -> PmiId {
    PmiId::mint(format!("sldprt:model:pmi#{id}")).expect("PMI ID")
}

fn face(key: u32) -> FaceId {
    FaceId::mint(format!("sldprt:brep:face#{key}")).expect("face ID")
}

fn tolerance(id: &str, kind: GeometricToleranceKind, magnitude: f64, target: u32) -> PmiAnnotation {
    PmiAnnotation {
        id: pmi(id),
        name: Some(id.into()),
        visible: None,
        targets: vec![PmiTarget::Face { face: face(target) }],
        definition: PmiDefinition::GeometricTolerance {
            tolerance: kind,
            magnitude: PmiMagnitude::from_parts(
                NonNegativeReal::new(magnitude).expect("magnitude"),
                PmiQuantity::Length,
            ),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: Vec::new(),
            constraints: ToleranceConstraints::default(),
        },
    }
}

fn datum(id: &str, letter: &str) -> PmiAnnotation {
    PmiAnnotation {
        id: pmi(id),
        name: None,
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::Datum {
            identification: letter.into(),
        },
    }
}

fn persistent(feature: u32, local: u32, tail: &[u32]) -> PersistentFaceIdentity {
    PersistentFaceIdentity {
        feature_source_id: FeatureSourceId::try_from(feature).expect("feature"),
        local_id: local,
        trailing_fields: tail.to_vec(),
    }
}

fn datum_letters(projected: &[PmiAnnotation], tolerance: &str) -> Vec<String> {
    let Some(PmiDefinition::GeometricTolerance {
        datum_system: Some(system),
        ..
    }) = projected
        .iter()
        .find(|annotation| annotation.id == pmi(tolerance))
        .map(|annotation| &annotation.definition)
    else {
        return Vec::new();
    };
    let Some(PmiDefinition::DatumSystem { references }) = projected
        .iter()
        .find(|annotation| &annotation.id == system)
        .map(|annotation| &annotation.definition)
    else {
        panic!("datum system")
    };
    references
        .as_slice()
        .iter()
        .map(|reference| {
            let Some(PmiDefinition::Datum { identification }) = projected
                .iter()
                .find(|annotation| annotation.id == reference.datum)
                .map(|annotation| &annotation.definition)
            else {
                panic!("datum")
            };
            identification.clone()
        })
        .collect()
}

#[test]
fn a_display_without_a_feature_path_depicts_the_tolerances_on_its_faces_and_supplies_tier_datums() {
    let image = Image {
        count: None,
        gtols: vec![
            Gtol {
                name: "DetailItem1",
                paths: vec![declared_reference(183, [1, 0, 0])],
                rows: &[
                    ("<GTOL-SPROF>", "2", &["D", "G", "H"]),
                    ("<GTOL-SPROF>", "0.2", &["D"]),
                ],
                note: "2 SURFACES",
            },
            Gtol {
                name: "DetailItem2",
                paths: vec![tagged_reference(44, [2, 0, 5], None)],
                rows: &[("<GTOL-FLAT>", ".01", &[])],
                note: "",
            },
        ],
        datum_tags: true,
        mirrors: &[],
    };
    let root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut projected = vec![
        tolerance("A1", GeometricToleranceKind::SurfaceProfile, 2.0, 10),
        tolerance(
            "A1:lower-tier",
            GeometricToleranceKind::SurfaceProfile,
            0.2,
            10,
        ),
        tolerance("A2", GeometricToleranceKind::Flatness, 0.254, 20),
        datum("D", "D"),
        datum("G", "G"),
        datum("H", "H"),
    ];
    let identities = [
        (face(10), persistent(183, 1, &[0, 0])),
        (face(20), persistent(44, 2, &[0, 5])),
    ];
    let archive = parse(&image.bytes());
    let (presentations, unresolved) =
        annotations(&archive, &root, &mut projected, &identities).expect("presentations");
    assert_eq!(unresolved, 0);
    let [composite, flatness] = presentations.as_slice() else {
        panic!("two presentations")
    };
    assert_eq!(
        depicted(composite),
        (
            Some("2 SURFACES"),
            vec!["sldprt:model:pmi#A1", "sldprt:model:pmi#A1:lower-tier"]
        )
    );
    assert_eq!(depicted(flatness), (None, vec!["sldprt:model:pmi#A2"]));
    assert_eq!(datum_letters(&projected, "A1"), ["D", "G", "H"]);
    assert_eq!(datum_letters(&projected, "A1:lower-tier"), ["D"]);
}

#[test]
fn a_face_identity_without_a_current_face_leaves_the_display_unlinked() {
    let image = Image {
        count: None,
        gtols: vec![
            Gtol {
                name: "DetailItem1",
                paths: vec![declared_reference(183, [1, 0, 0])],
                rows: &[("<GTOL-FLAT>", ".01", &[])],
                note: "",
            },
            Gtol {
                name: "DetailItem2",
                paths: vec![tagged_reference(44, [2, 0, 5], None)],
                rows: &[("<GTOL-FLAT>", ".01", &[])],
                note: "",
            },
        ],
        datum_tags: true,
        mirrors: &[],
    };
    let root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    let mut projected = vec![tolerance("A2", GeometricToleranceKind::Flatness, 0.254, 20)];
    let identities = [(face(20), persistent(44, 2, &[0]))];
    let archive = parse(&image.bytes());
    let (_, unresolved) =
        annotations(&archive, &root, &mut projected, &identities).expect("presentations");
    assert_eq!(unresolved, 2);
}
