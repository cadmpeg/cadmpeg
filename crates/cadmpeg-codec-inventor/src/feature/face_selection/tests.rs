// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::tests::{test_feature, test_property};
use crate::pmdc::{PmDcListMetadata, PmDcReferenceList, PmDcU32List};
use cadmpeg_ir::features::{FaceMotion, FeatureEvaluation};

const SELECTOR_SOURCE: u32 = 9;
const SELECTOR_KEY: u32 = 7;

/// Fields of a synthetic face selector.
struct Fields {
    state: u32,
    form: u8,
    topology: u32,
    qualifier: u32,
    naming: u32,
    generators: Vec<u32>,
    secondary: Vec<u32>,
    suffix: u8,
    context: u32,
    key: u32,
    entities: Vec<(u32, u32)>,
    count: u32,
}

impl Fields {
    fn new() -> Self {
        Self {
            state: 0,
            form: 5,
            topology: 2,
            qualifier: 0,
            naming: 2,
            generators: vec![31, 37],
            secondary: vec![3],
            suffix: 0,
            context: 0x8000_0004,
            key: SELECTOR_KEY,
            entities: vec![(0x8000_0004, 11), (0x8000_0004, 13)],
            count: 2,
        }
    }

    fn bytes(&self, version: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        for word in [0_u32, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if version >= 28 {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        for word in [0x8000_0003_u32, SELECTOR_SOURCE, self.state] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, self.form]);
        for word in [0x8000_000a_u32, self.topology] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        if version != 19 {
            bytes.extend_from_slice(&self.qualifier.to_le_bytes());
        }
        bytes.extend_from_slice(&self.naming.to_le_bytes());
        for values in [&self.generators, &self.secondary] {
            bytes.extend_from_slice(&8_u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
            bytes.extend_from_slice(
                &u32::try_from(values.len())
                    .expect("small list")
                    .to_le_bytes(),
            );
            if !values.is_empty() {
                bytes.extend_from_slice(&[8, 0, 0, 0]);
            }
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(self.suffix);
        for word in [self.context, self.key, self.count] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for (context, entity) in &self.entities {
            bytes.extend_from_slice(&context.to_le_bytes());
            bytes.extend_from_slice(&entity.to_le_bytes());
        }
        bytes.extend_from_slice(&0x8000_0008_u32.to_le_bytes());
        bytes
    }
}

fn context() -> DecodeContext<'static> {
    // The arena outlives every test context.
    let arena: &'static cadmpeg_core::decode::DecodeArena =
        Box::leak(Box::new(cadmpeg_core::decode::DecodeArena::new()));
    DecodeContext::from_root_bytes(&[], arena, &cadmpeg_core::decode::DecodePolicy::service())
        .expect("context")
        .0
}

fn parse_bytes(bytes: &[u8], version: u8) -> Result<Option<FaceSelector>, CodecError> {
    parse(&context(), View::over_retained(bytes), version)
}

#[test]
fn a_face_selector_stores_generators_a_key_entities_and_its_face_item() {
    for version in [19, 23, 29] {
        let bytes = Fields::new().bytes(version);
        let selector = parse_bytes(&bytes, version)
            .expect("framed")
            .expect("face selector");
        assert_eq!(selector.source, SELECTOR_SOURCE);
        assert_eq!(selector.key, SELECTOR_KEY);
        assert_eq!(selector.generators, [31, 37]);
        assert_eq!(selector.item.record_ordinal(), Some(7));
        assert_eq!(selector.body.record_ordinal(), Some(9));
        assert_eq!(selector.context.record_ordinal(), Some(3));
        let mut empty = Fields::new();
        empty.entities.clear();
        empty.count = 0;
        assert!(parse_bytes(&empty.bytes(version), version)
            .expect("framed")
            .is_some());
        assert!(parse_bytes(&bytes[..bytes.len() - 1], version)
            .expect("framed refusal")
            .is_none());
    }
}

#[test]
fn the_topology_words_equal_the_source_index_of_the_selected_body() {
    for version in [19, 23, 29] {
        let mut fields = Fields::new();
        fields.topology = 74;
        fields.naming = 74;
        let selector = parse_bytes(&fields.bytes(version), version)
            .expect("framed")
            .expect("face selector");
        let target = |ordinal| (ordinal == 9).then_some(BODY_TYPE);
        let context = |ordinal| match ordinal {
            9 => Some(BODY_TYPE),
            7 => Some(crate::feature::FACE_ITEM_TYPE),
            3 => Some(CONTEXT_TYPE),
            _ => target(ordinal),
        };
        assert!(links(&selector, context, |ordinal| (ordinal == 9).then_some(74)));
        assert!(!links(&selector, context, |ordinal| (ordinal == 9).then_some(2)));
        // Unequal words never parse.
        fields.naming = 2;
        assert!(parse_bytes(&fields.bytes(version), version)
            .expect("framed refusal")
            .is_none());
    }
}

#[test]
fn face_selector_forms_that_differ_in_any_control_stay_unparsed() {
    type Edit = fn(&mut Fields);
    let edits: [(&str, Edit); 14] = [
        ("state", |f| f.state = u32::MAX),
        ("form", |f| f.form = 6),
        ("topology", |f| f.topology = 3),
        ("qualifier", |f| f.qualifier = 1),
        ("naming", |f| f.naming = 3),
        ("no generator", |f| f.generators.clear()),
        ("zero generator", |f| f.generators = vec![0]),
        ("zero secondary", |f| f.secondary = vec![0]),
        ("suffix", |f| f.suffix = 1),
        ("zero key", |f| f.key = 0),
        ("count above entities", |f| f.count = 3),
        ("count below entities", |f| f.count = 1),
        ("foreign entity context", |f| f.entities[1].0 = 0x8000_0005),
        ("zero entity", |f| f.entities[0].1 = 0),
    ];
    for version in [19, 23, 29] {
        for (name, edit) in edits {
            if name == "qualifier" && version == 19 {
                continue;
            }
            let mut fields = Fields::new();
            edit(&mut fields);
            assert!(
                parse_bytes(&fields.bytes(version), version)
                    .expect("framed refusal")
                    .is_none(),
                "{name} at version {version}"
            );
        }
    }
}

#[test]
fn face_selector_references_require_qualified_typed_targets() {
    let bytes = Fields::new().bytes(29);
    for change in 0..8 {
        let mut selector = parse_bytes(&bytes, 29)
            .expect("framed")
            .expect("face selector");
        match change {
            1 => selector.body = PmDcReference::from_packed(10),
            2 => selector.item = PmDcReference::from_packed(8),
            3 => selector.context = PmDcReference::from_packed(4),
            _ => {}
        }
        let target = |ordinal| match ordinal {
            9 if change != 4 => Some(BODY_TYPE),
            7 if change != 5 => Some(crate::feature::FACE_ITEM_TYPE),
            3 if change != 6 => Some(CONTEXT_TYPE),
            _ => None,
        };
        let body_source = |ordinal| (ordinal == 9).then_some(if change == 7 { 3 } else { 2 });
        assert_eq!(
            links(&selector, target, body_source),
            change == 0,
            "change {change}"
        );
    }
}

fn record(index: usize, name: &str, tokens: Vec<Token>) -> Record {
    Record {
        index,
        name: name.into(),
        tokens: tokens.into(),
        offset: 0,
        len: 0,
    }
}

fn common(owner: i64) -> Vec<Token> {
    vec![
        Token::Ref(-1),
        Token::Long(-1),
        Token::Ref(-1),
        Token::Ref(-1),
        Token::Ref(owner),
    ]
}

fn matched(index: usize, owner: i64, schema: u32, source: i64, key: i64, qualifier: i64) -> Record {
    let mut tokens = common(owner);
    if schema == 44 {
        tokens.extend([
            Token::Str("INV_NMX_MATCHED_ATTRIB".into()),
            Token::Long(1),
            Token::Long(44),
        ]);
        tokens.extend([1, source, -1, key, qualifier].map(Token::Long));
        record(index, "ATTRIB_CUSTOM-attrib", tokens)
    } else {
        tokens.push(Token::Long(i64::from(schema)));
        tokens.extend([1, source, -1, key, qualifier].map(Token::Long));
        record(index, "NMx_Matched_Entity-NamingMatching-attrib", tokens)
    }
}

fn generated(index: usize, owner: i64, schema: u32, generator: i64) -> Record {
    let mut tokens = common(owner);
    if schema == 44 {
        tokens.extend([
            Token::Str("INV_NMX_GEN_TAG".into()),
            Token::Long(1),
            Token::Long(44),
        ]);
        tokens.extend([1, generator, -1, 23, 0, 20].map(Token::Long));
        record(index, "ATTRIB_CUSTOM-attrib", tokens)
    } else {
        tokens.push(Token::Long(i64::from(schema)));
        tokens.extend([1, generator, -1, 23, 0, 20].map(Token::Long));
        record(
            index,
            "NMx_Generated_Brep_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
            tokens,
        )
    }
}

fn face(index: usize) -> Record {
    record(index, "face", vec![Token::Ref(-1), Token::Long(-1)])
}

fn selector() -> FaceSelector {
    parse_bytes(&Fields::new().bytes(19), 19)
        .expect("framed")
        .expect("face selector")
}

fn archive(schema: u32) -> Vec<Record> {
    vec![
        face(1),
        matched(10, 1, schema, 9, 7, 1),
        generated(11, 1, schema, 31),
    ]
}

fn run(records: &[Record], schema: u32) -> Option<SelectedFace> {
    selected(records, &[selector()], schema).remove(&SELECTOR_SOURCE)
}

#[test]
fn a_matched_face_owner_with_the_face_qualifier_and_a_shared_generator_is_selected() {
    for schema in [38, 40, 42, 44] {
        let selected = run(&archive(schema), schema).expect("selected face");
        assert_eq!((selected.owner, selected.item), (1, 7));
        // A face without a generated name is selected by the join alone.
        let mut bare = archive(schema);
        bare.pop();
        assert_eq!(run(&bare, schema).expect("selected").owner, 1);
        // The second generator also names the face.
        let mut second = archive(schema);
        second[2] = generated(11, 1, schema, 37);
        assert!(run(&second, schema).is_some());
        // A history snapshot of a face is a face revision.
        let mut snapshot = archive(schema);
        snapshot[0] = record(
            1,
            "End-of-ASM-History-Section",
            vec![Token::Ident("face".into()), Token::Ref(-1)],
        );
        assert!(run(&snapshot, schema).is_some());
    }
}

/// A generated name of a second family, the blend family.
fn blend(index: usize, owner: i64, schema: u32, generator: i64) -> Record {
    let mut tokens = common(owner);
    if schema == 44 {
        tokens.extend([
            Token::Str("INV_NMX_BLEND_TAG".into()),
            Token::Long(1),
            Token::Long(44),
        ]);
        tokens.extend([1, generator, -1, 23, 0, 20].map(Token::Long));
        record(index, "ATTRIB_CUSTOM-attrib", tokens)
    } else {
        tokens.push(Token::Long(i64::from(schema)));
        tokens.extend([1, generator, -1, 1, 2, 3, 4, 5, 6, 7].map(Token::Long));
        record(
            index,
            "NMx_blend_tag-NMx_Brep_Name_tag-NMx_Brep_tag-NamingMatching-attrib",
            tokens,
        )
    }
}

#[test]
fn a_schema_43_carrier_joins_no_face() {
    let records = archive(38)
        .into_iter()
        .map(|mut record| {
            let mut tokens = record.tokens.to_vec();
            for token in &mut tokens {
                if matches!(token, Token::Long(38)) {
                    *token = Token::Long(43);
                }
            }
            record.tokens = tokens.into();
            record
        })
        .collect::<Vec<_>>();
    assert!(crate::feature::edge_selection::matched_qualified(&records[1], 43).is_some());
    assert!(run(&records, 43).is_none());
}

#[test]
fn a_plain_join_states_no_assumption() {
    for schema in [38, 44] {
        let face = run(&archive(schema), schema).expect("selected");
        assert!(!face.repeated_attribute && !face.unshared_family);
        // Two families that both share a generator.
        let mut both = archive(schema);
        both.push(blend(12, 1, schema, 37));
        let face = run(&both, schema).expect("selected");
        assert!(!face.repeated_attribute && !face.unshared_family);
        // Two names of one family, of which one shares a generator.
        let mut one_family = archive(schema);
        one_family.push(generated(12, 1, schema, 99));
        let face = run(&one_family, schema).expect("selected");
        assert!(!face.unshared_family);
    }
}

#[test]
fn a_repeated_attribute_on_one_owner_selects_it_and_states_the_repeat() {
    for schema in [38, 44] {
        let mut copies = archive(schema);
        copies.push(matched(12, 1, schema, 9, 7, 1));
        let face = run(&copies, schema).expect("selected");
        assert_eq!(face.owner, 1);
        assert!(face.repeated_attribute && !face.unshared_family);
        // A copy on a second owner still selects nothing.
        let mut owners = archive(schema);
        owners.push(face_record_with_attribute(schema));
        assert!(run(&owners, schema).is_none());
    }
}

fn face_record_with_attribute(schema: u32) -> Record {
    matched(12, 2, schema, 9, 7, 1)
}

#[test]
fn a_family_that_shares_no_generator_selects_through_another_and_states_it() {
    for schema in [38, 44] {
        let mut mixed = archive(schema);
        mixed.push(blend(12, 1, schema, 99));
        let face = run(&mixed, schema).expect("selected");
        assert_eq!(face.owner, 1);
        assert!(face.unshared_family && !face.repeated_attribute);
        // No family shares a generator: nothing is selected.
        let mut none = archive(schema);
        none[2] = generated(11, 1, schema, 98);
        none.push(blend(12, 1, schema, 99));
        assert!(run(&none, schema).is_none());
    }
}

#[test]
fn assumption_losses_count_the_features_that_use_the_face() {
    let ctx = context();
    let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    assert!(assumption_losses(std::slice::from_ref(&feature), &inventory).is_empty());
    {
        let face = inventory
            .edge_inputs
            .faces
            .get_mut(&(TOKEN.to_owned(), 41))
            .expect("first face");
        face.repeated_attribute = true;
        face.unshared_family = true;
    }
    // A native selection uses no face, so no assumption applies.
    assert!(assumption_losses(std::slice::from_ref(&feature), &inventory).is_empty());
    project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    let notes = assumption_losses(std::slice::from_ref(&feature), &inventory);
    assert_eq!(notes.len(), 2);
}

#[test]
fn a_face_join_refuses_other_owners_qualifiers_keys_names_and_schemas() {
    for schema in [38, 44] {
        let base = archive(schema);
        let mut cases: Vec<(&str, Vec<Record>)> = Vec::new();
        let mut edge_owner = base.clone();
        edge_owner[0] = record(1, "edge", vec![Token::Ref(-1)]);
        cases.push(("edge owner", edge_owner));
        for qualifier in [0, 2] {
            let mut changed = base.clone();
            changed[1] = matched(10, 1, schema, 9, 7, qualifier);
            cases.push(("qualifier", changed));
        }
        let mut key = base.clone();
        key[1] = matched(10, 1, schema, 9, 8, 1);
        cases.push(("key", key));
        let mut source = base.clone();
        source[1] = matched(10, 1, schema, 10, 7, 1);
        cases.push(("source", source));
        let mut disjoint = base.clone();
        disjoint[2] = generated(11, 1, schema, 99);
        cases.push(("disjoint generated name", disjoint));
        // The same owner with two qualifiers is not one face match.
        let mut mixed = base.clone();
        mixed.push(matched(12, 1, schema, 9, 7, 0));
        cases.push(("two qualifiers", mixed));
        let mut second = base.clone();
        second.insert(1, face(2));
        second.push(matched(12, 2, schema, 9, 7, 1));
        cases.push(("two owners", second));
        let mut missing = base.clone();
        missing.remove(0);
        cases.push(("owner absent", missing));
        let mut unmatched = base.clone();
        unmatched.remove(1);
        cases.push(("no matching attribute", unmatched));
        for (name, records) in cases {
            assert!(run(&records, schema).is_none(), "{name} at schema {schema}");
        }
        // The attribute of another carrier schema does not join.
        let other = if schema == 44 { 38 } else { 44 };
        assert!(run(&base, other).is_none(), "carrier schema {other}");
    }
}

#[test]
fn an_edge_matching_attribute_keeps_the_edge_join_and_ignores_the_face_join() {
    let edge = record(1, "edge", vec![Token::Ref(-1)]);
    let attribute = matched(10, 1, 44, 9, 7, 0);
    assert_eq!(
        crate::feature::edge_selection::matched_qualified(&attribute, 44),
        Some((9, 7, 1, 0))
    );
    assert!(run(&[edge, attribute], 44).is_none());
}

const TOKEN: &str = "generated";

fn reference(index: u32) -> PmDcReference {
    PmDcReference::new(index, true).expect("valid synthetic reference")
}

fn reference_list(ordinals: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!ordinals.is_empty()).then_some(PmDcListMetadata::U32([
            u32::try_from(ordinals.len()).expect("small list"),
            0,
        ])),
        ordinals
            .iter()
            .map(|ordinal| reference(ordinal + 1))
            .collect(),
    )
    .expect("valid synthetic list")
}

fn face_item(ordinal: u32, sources: &[u32]) -> PmDcFeatureProperty {
    test_property(
        ordinal,
        PmDcFeaturePropertyKind::FaceItem {
            index_references: PmDcU32List::new(
                2,
                (!sources.is_empty()).then_some(PmDcListMetadata::U32([
                    u32::try_from(sources.len()).expect("small list"),
                    0,
                ])),
                sources.to_vec(),
            )
            .expect("valid synthetic list"),
        },
    )
}

/// A Move Face feature with a collection of items 21 and 22, selecting
/// selector sources 41 and 43.
fn fixture(
    ctx: &DecodeContext<'_>,
    sources: [&[u32]; 2],
) -> (crate::feature::FeatureInventory, Feature) {
    let source = test_feature(10, 1, &[(0, 20)]);
    let properties = vec![
        test_property(
            20,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[21, 22]),
            },
        ),
        face_item(21, sources[0]),
        face_item(22, sources[1]),
    ];
    let mut feature =
        crate::feature::native_feature(ctx, &source.identity, None, source.id()).expect("feature");
    feature.source_tag = Some("move_face".into());
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces: FaceSelection::Native(properties[0].id()),
            motion: FaceMotion::Translate {
                direction: cadmpeg_ir::features::FeatureDirection3::from(
                    cadmpeg_ir::units::UnitVector3::X_AXIS,
                ),
                distance: cadmpeg_ir::scalar::Length::new(2.0).expect("length"),
            },
        },
    ));
    let mut inventory = crate::feature::FeatureInventory {
        features: vec![source],
        properties,
        ..crate::feature::FeatureInventory::default()
    };
    let token = inventory.features[0]
        .identity
        .segment_token
        .as_str()
        .to_owned();
    for (selector, item, owner) in [(41, 21, 5), (43, 22, 6)] {
        inventory.edge_inputs.faces.insert(
            (token.clone(), selector),
            SelectedFace {
                owner,
                item,
                repeated_attribute: false,
                unshared_family: false,
            },
        );
    }
    (inventory, feature)
}

fn faces(feature: &Feature) -> &FaceSelection {
    selection(feature).expect("Move Face")
}

fn check(
    inventory: &crate::feature::FeatureInventory,
    feature: &Feature,
    states: &[FeatureInputTopology],
) -> bool {
    valid(
        feature,
        &inventory.features[0],
        states,
        |id| inventory.properties.iter().find(|p| p.id() == id),
        |r| {
            inventory
                .properties
                .iter()
                .find(|p| Some(p.identity.record_ordinal) == r.record_ordinal())
        },
    )
}

#[test]
fn every_joined_face_operand_replaces_the_native_collection_in_one_input_state() {
    let ctx = context();
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].faces.len(), 2);
    assert!(states[0].edges.is_empty() && states[0].edge_geometry.is_empty());
    let FaceSelection::Historical {
        state,
        faces: selected,
        native,
    } = faces(&feature)
    else {
        panic!("historical faces")
    };
    assert_eq!(*state, states[0].id);
    assert_eq!(native.as_str(), inventory.properties[0].id());
    assert_eq!(
        selected
            .as_slice()
            .iter()
            .map(HistoricalFaceId::as_str)
            .collect::<Vec<_>>(),
        [
            format!("inventor:input-face#{TOKEN}-41"),
            format!("inventor:input-face#{TOKEN}-43")
        ]
    );
    assert!(check(&inventory, &feature, &states));
    assert!(native_losses(std::slice::from_ref(&feature)).is_empty());
}

#[test]
fn a_partly_joined_collection_keeps_its_unresolved_items_native() {
    let ctx = context();
    for missing in [41, 43] {
        let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        inventory
            .edge_inputs
            .faces
            .remove(&(TOKEN.to_owned(), missing));
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        assert_eq!(states.len(), 1);
        let FaceSelection::HistoricalPartial {
            faces: selected,
            unresolved,
            ..
        } = faces(&feature)
        else {
            panic!("partial faces")
        };
        assert_eq!(selected.len(), 1);
        let item = if missing == 41 { 1 } else { 2 };
        assert_eq!(unresolved.as_slice(), [inventory.properties[item].id()]);
        assert!(check(&inventory, &feature, &states));
        assert_eq!(native_losses(std::slice::from_ref(&feature)).len(), 1);
    }
}

#[test]
fn faces_that_join_nothing_or_collide_stay_native_with_a_loss() {
    let ctx = context();
    for change in 0..4 {
        let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        match change {
            0 => inventory.edge_inputs.faces.clear(),
            // Two operands that join one face revision.
            1 => {
                inventory
                    .edge_inputs
                    .faces
                    .get_mut(&(TOKEN.to_owned(), 43))
                    .expect("second face")
                    .owner = 5;
            }
            // Selectors listed by another face item do not join.
            2 => {
                for face in inventory.edge_inputs.faces.values_mut() {
                    face.item = 99;
                }
            }
            // Face items that list no selector.
            _ => {
                let (other, _) = fixture(&ctx, [&[], &[]]);
                inventory.properties = other.properties;
            }
        }
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        assert!(states.is_empty(), "change {change}");
        assert!(matches!(faces(&feature), FaceSelection::Native(_)));
        assert_eq!(native_losses(std::slice::from_ref(&feature)).len(), 1);
    }
}

#[test]
fn a_repeated_selector_source_in_a_collection_stays_native() {
    let ctx = context();
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[41]]);
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

#[test]
fn a_stored_face_selection_must_re_derive_from_its_collection_and_state() {
    let ctx = context();
    {
        let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        assert!(check(&inventory, &feature, &states));
    }
    for change in 0..6 {
        let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        let mut states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        match change {
            0 => states.clear(),
            1 => states[0].native_ref = Some("other".into()),
            2 => {
                states[0].faces = vec![states[0].faces.iter().next().expect("face").clone()]
                    .try_into()
                    .expect("one face");
            }
            3 => {
                let mut row = states[0].clone();
                row.id = FeatureInputTopologyId::compose(
                    &cadmpeg_ir::identity_namespace!("inventor", "design", "feature-input"),
                    inventory.features[0].identity.key().expect("key"),
                );
                states.push(row);
            }
            4 => {
                inventory.properties[1] = face_item(21, &[44]);
            }
            _ => {
                feature.evaluation = FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::MoveFace {
                        faces: FaceSelection::historical(
                            states[0].id.clone(),
                            vec![
                                face_id(TOKEN, 43).expect("id"),
                                face_id(TOKEN, 41).expect("id"),
                            ],
                            inventory.properties[0].id(),
                        )
                        .expect("selection"),
                        motion: FaceMotion::Translate {
                            direction: cadmpeg_ir::features::FeatureDirection3::from(
                                cadmpeg_ir::units::UnitVector3::X_AXIS,
                            ),
                            distance: cadmpeg_ir::scalar::Length::new(2.0).expect("length"),
                        },
                    }),
                );
            }
        }
        // A duplicated row is rejected; a missing or edited one fails.
        assert!(!check(&inventory, &feature, &states), "change {change}");
    }
}

#[test]
fn an_empty_face_item_selects_no_face_and_an_all_empty_collection_stays_native() {
    let ctx = context();
    // An empty item beside a joined item leaves the joined face as the selection.
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[]]);
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert_eq!(states.len(), 1);
    let FaceSelection::Historical {
        faces: selected, ..
    } = faces(&feature)
    else {
        panic!("historical faces")
    };
    assert_eq!(selected.as_slice().len(), 1);
    assert!(check(&inventory, &feature, &states));
    // A collection of empty items selects no face.
    let (inventory, mut feature) = fixture(&ctx, [&[], &[]]);
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

#[test]
fn only_face_motion_features_take_the_face_join() {
    let ctx = context();
    for tag in [None, Some("thicken")] {
        let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        feature.source_tag = tag.map(str::to_owned);
        assert!(
            project(&ctx, std::slice::from_mut(&mut feature), &inventory)
                .expect("project")
                .is_empty()
        );
        assert!(native_losses(std::slice::from_ref(&feature)).is_empty());
    }
}
