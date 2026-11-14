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
        if !(19..=21).contains(&version) {
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
    for version in [19, 20, 21, 23, 29] {
        for (name, edit) in edits {
            if name == "qualifier" && (19..=21).contains(&version) {
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
fn a_schema_43_face_join_checks_the_generated_name_and_carrier_schema() {
    let records = archive(43);
    assert!(run(&records, 43).is_some());
    let mut foreign = records.clone();
    foreign[2] = generated(11, 1, 43, 99);
    assert!(run(&foreign, 43).is_none());
    assert!(run(&records, 42).is_none());
    assert!(run(&archive(45), 45).is_none());
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
    assert_eq!(notes.len(), 3);
    assert_eq!(
        notes[0].code.local_code(),
        crate::loss::InventorLossCode::FeatureFaceJoinInputRevisionAssumed.code()
    );
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
            selected_face(owner, item, Surface::Other),
        );
    }
    (inventory, feature)
}

fn selected_face(owner: i64, item: u32, surface: Surface) -> SelectedFace {
    SelectedFace {
        schema: 42,
        owner,
        owners: vec![owner],
        generators: 1,
        item,
        repeated_attribute: false,
        unshared_family: false,
        identical_owners: false,
        form: SelectorForm::Naming,
        surface,
        state: 0,
        feature_sources: Vec::new(),
    }
}

fn faces(feature: &Feature) -> &FaceSelection {
    selection(feature).expect("face-selecting feature").1
}

fn check(
    inventory: &crate::feature::FeatureInventory,
    feature: &Feature,
    states: &[FeatureInputTopology],
) -> bool {
    check_with_schema(inventory, feature, states, 42)
}

fn check_with_schema(
    inventory: &crate::feature::FeatureInventory,
    feature: &Feature,
    states: &[FeatureInputTopology],
    schema: u32,
) -> bool {
    valid(
        feature,
        &inventory.features[0],
        states,
        &crate::feature::edge_selection::historical::tests::carrier(schema),
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
fn a_direct_face_selector_joins_no_face_selecting_feature() {
    let ctx = context();
    // Move Face keeps no joined face and no partial state for a direct selector.
    for direct in [[true, true], [true, false], [false, true]] {
        let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
        for (selector, direct) in [41, 43].into_iter().zip(direct) {
            inventory
                .edge_inputs
                .faces
                .get_mut(&(TOKEN.to_owned(), selector))
                .expect("selected")
                .form = if direct {
                SelectorForm::Direct
            } else {
                SelectorForm::Naming
            };
        }
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        let joined = states.first().map_or(0, |state| state.faces.len());
        assert_eq!(joined, 2 - direct.iter().filter(|direct| **direct).count());
    }
    // A Thicken face list that names a direct selector does not join.
    let (mut inventory, mut feature) = thicken_fixture(&ctx);
    for face in inventory.edge_inputs.faces.values_mut() {
        face.form = SelectorForm::Direct;
    }
    let before = feature.clone();
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert!(states.is_empty());
    assert_eq!(feature, before);
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

/// Replaces the Move Face operation of a fixture by `definition` under `tag`.
fn retarget(feature: &mut Feature, tag: &str, definition: FeatureOperation) {
    feature.source_tag = Some(tag.into());
    feature.evaluation =
        FeatureEvaluation::from_definition(FeatureDefinition::Operation(definition));
}

fn shell(native: String) -> FeatureOperation {
    FeatureOperation::Shell {
        bodies: None,
        removed_faces: FaceSelection::Native(native),
        thickness: cadmpeg_ir::scalar::PositiveLength::new(2.0),
        outward: None,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    }
}

#[test]
fn shell_removed_faces_join_like_a_face_collection_and_keep_no_partial_selection() {
    let ctx = context();
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    retarget(&mut feature, "shell", shell(inventory.properties[0].id()));
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].faces.len(), 2);
    assert!(matches!(
        faces(&feature),
        FaceSelection::Historical { native, .. } if native.as_str() == inventory.properties[0].id()
    ));
    assert!(check(&inventory, &feature, &states));
    // One unresolved removed face leaves the whole selection native.
    let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    retarget(&mut feature, "shell", shell(inventory.properties[0].id()));
    inventory.edge_inputs.faces.remove(&(TOKEN.to_owned(), 43));
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
    // A shell without the Shell source tag takes no join.
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    retarget(&mut feature, "other", shell(inventory.properties[0].id()));
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
}

fn thread(native: String, diameter: Option<f64>) -> FeatureOperation {
    FeatureOperation::CosmeticThread {
        face: FaceSelection::Native(native),
        standard: None,
        designation: None,
        diameter: diameter.and_then(cadmpeg_ir::scalar::PositiveLength::new),
        pitch: None,
        modeled: None,
        extent: None,
    }
}

/// A thread annotation that selects the face of selector 41 on a surface.
fn thread_fixture(
    ctx: &DecodeContext<'_>,
    surface: Surface,
    diameter: Option<f64>,
) -> (crate::feature::FeatureInventory, Feature) {
    let (mut inventory, mut feature) = fixture(ctx, [&[41], &[]]);
    retarget(
        &mut feature,
        "thread",
        thread(inventory.properties[0].id(), diameter),
    );
    inventory
        .edge_inputs
        .faces
        .insert((TOKEN.to_owned(), 41), selected_face(5, 21, surface));
    (inventory, feature)
}

#[test]
fn a_thread_face_within_a_tolerance_states_the_assumption() {
    let ctx = context();
    let surface = |ratio, sine| Surface::Cone {
        ratio,
        sine,
        radius: 0.25,
    };
    for (face, diameter, assumed) in [
        (surface(1.0, 0.0), Some(5.0), false),
        (surface(1.0, 0.0), None, false),
        (surface(1.0, 0.1), Some(9.0), false),
        (surface(1.000_000_000_5, 0.0), Some(5.0), true),
        (surface(1.0, 5e-10), Some(5.0), true),
        (surface(1.0, 0.0), Some(5.000_000_5), true),
    ] {
        let (inventory, mut feature) = thread_fixture(&ctx, face, diameter);
        let before = assumption_losses(std::slice::from_ref(&feature), &inventory);
        assert!(before.is_empty(), "a native selection uses no face");
        project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        let notes = assumption_losses(std::slice::from_ref(&feature), &inventory);
        assert_eq!(
            notes
                .iter()
                .any(|note| note.code.local_code() == "feature.face-join-thread-tolerance-assumed"),
            assumed,
            "{face:?} {diameter:?}"
        );
    }
}

#[test]
fn a_thread_face_with_a_non_finite_cone_ratio_names_it() {
    // The record keeps the surface a cone.
    let mut records = archive(44);
    records[0] = face_on(1, 3, false);
    let mut nan_cone = cone(3, 0.0, 0.25);
    let mut tokens = nan_cone.tokens.to_vec();
    tokens[6] = Token::Double(f64::NAN);
    nan_cone.tokens = tokens.into();
    records.push(nan_cone);
    let selected = run(&records, 44).expect("selected");
    assert!(matches!(selected.surface, Surface::Cone { ratio, .. } if ratio.is_nan()));
    // A finite ratio outside the tolerance is not a cone.
    let mut far = records.clone();
    let mut wide = cone(3, 0.0, 0.25);
    let mut tokens = wide.tokens.to_vec();
    tokens[6] = Token::Double(1.1);
    wide.tokens = tokens.into();
    let last = far.len() - 1;
    far[last] = wide;
    assert_eq!(run(&far, 44).expect("selected").surface, Surface::Other);
    // The note names the value, and the tolerance note does not claim it.
    let ctx = context();
    for ratio in [f64::NAN, f64::INFINITY] {
        let surface = Surface::Cone {
            ratio,
            sine: 0.0,
            radius: 0.25,
        };
        let (inventory, mut feature) = thread_fixture(&ctx, surface, Some(5.0));
        project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        let notes = assumption_losses(std::slice::from_ref(&feature), &inventory);
        let codes = notes
            .iter()
            .map(|note| note.code.local_code())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"feature.face-join-thread-ratio-nonfinite"));
        assert!(!codes.contains(&"feature.face-join-thread-tolerance-assumed"));
    }
}

#[test]
fn a_thread_joins_one_cylinder_or_cone_of_the_stated_size() {
    let ctx = context();
    // The cylinder radius is in archive length units; the diameter is in millimetres.
    let cylinder = Surface::Cone {
        ratio: 1.0,
        sine: 0.0,
        radius: 0.25,
    };
    let taper = Surface::Cone {
        ratio: 1.0,
        sine: 0.1,
        radius: 0.25,
    };
    for (surface, diameter, joins) in [
        (cylinder, Some(5.0), true),
        (cylinder, None, true),
        (cylinder, Some(5.5), false),
        (cylinder, Some(2.5), false),
        (taper, Some(9.0), true),
        (Surface::Plane, Some(5.0), false),
        (Surface::Other, None, false),
    ] {
        let (inventory, mut feature) = thread_fixture(&ctx, surface, diameter);
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        assert_eq!(states.len(), usize::from(joins), "{surface:?} {diameter:?}");
        assert_eq!(
            matches!(faces(&feature), FaceSelection::Historical { .. }),
            joins
        );
        if joins {
            assert!(check(&inventory, &feature, &states));
        }
    }
}

#[test]
fn a_thread_selects_exactly_one_face() {
    let ctx = context();
    let cylinder = Surface::Cone {
        ratio: 1.0,
        sine: 0.0,
        radius: 0.25,
    };
    let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    retarget(
        &mut feature,
        "thread",
        thread(inventory.properties[0].id(), None),
    );
    for (selector, item, owner) in [(41, 21, 5), (43, 22, 6)] {
        inventory.edge_inputs.faces.insert(
            (TOKEN.to_owned(), selector),
            selected_face(owner, item, cylinder),
        );
    }
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
    // A stored two-face thread selection does not validate.
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    let mut motion = fixture(&ctx, [&[41], &[43]]).1;
    let states = project(&ctx, std::slice::from_mut(&mut motion), &inventory).expect("project");
    let FaceSelection::Historical {
        state,
        faces,
        native,
    } = faces(&motion).clone()
    else {
        panic!("historical faces")
    };
    retarget(
        &mut feature,
        "thread",
        FeatureOperation::CosmeticThread {
            face: FaceSelection::Historical {
                state,
                faces,
                native,
            },
            standard: None,
            designation: None,
            diameter: None,
            pitch: None,
            modeled: None,
            extent: None,
        },
    );
    assert!(!check(&inventory, &feature, &states));
}

/// A Thicken feature whose selection list holds two wrappers. Each wrapper
/// names a face collection of one face item.
fn thicken_fixture(ctx: &DecodeContext<'_>) -> (crate::feature::FeatureInventory, Feature) {
    let source = test_feature(10, 1, &[(0, 30)]);
    let properties = vec![
        test_property(
            30,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::SelectionList,
                items: reference_list(&[31, 33]),
            },
        ),
        test_property(
            31,
            PmDcFeaturePropertyKind::ThickenSelection {
                faces: reference(33),
                parameter: reference(60),
            },
        ),
        test_property(
            32,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[34]),
            },
        ),
        test_property(
            33,
            PmDcFeaturePropertyKind::ThickenSelection {
                faces: reference(37),
                parameter: reference(60),
            },
        ),
        face_item(34, &[41]),
        test_property(
            36,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::FaceCollection,
                items: reference_list(&[37]),
            },
        ),
        face_item(37, &[43]),
    ];
    let mut feature =
        crate::feature::native_feature(ctx, &source.identity, None, source.id()).expect("feature");
    retarget(
        &mut feature,
        "thicken",
        FeatureOperation::thicken(
            FaceSelection::Native(properties[0].id()),
            cadmpeg_ir::scalar::PositiveLength::new(1.0),
            None,
        ),
    );
    let mut inventory = crate::feature::FeatureInventory {
        features: vec![source],
        properties,
        ..crate::feature::FeatureInventory::default()
    };
    for (selector, item, owner) in [(41, 34, 5), (43, 37, 6)] {
        inventory.edge_inputs.faces.insert(
            (TOKEN.to_owned(), selector),
            selected_face(owner, item, Surface::Other),
        );
    }
    (inventory, feature)
}

#[test]
fn a_thicken_selection_list_joins_the_faces_of_every_wrapper() {
    let ctx = context();
    let (inventory, mut feature) = thicken_fixture(&ctx);
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert_eq!(states.len(), 1);
    let FaceSelection::Historical {
        faces: selected,
        native,
        ..
    } = faces(&feature)
    else {
        panic!("historical faces")
    };
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
    // A wrapper that does not name a face collection leaves the list native.
    let (mut inventory, mut feature) = thicken_fixture(&ctx);
    inventory.properties[1] = test_property(
        31,
        PmDcFeaturePropertyKind::ThickenSelection {
            faces: reference(31),
            parameter: reference(60),
        },
    );
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    // One unresolved face leaves the list native: a Thicken has no partial selection.
    let (mut inventory, mut feature) = thicken_fixture(&ctx);
    inventory.edge_inputs.faces.remove(&(TOKEN.to_owned(), 43));
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

fn offset_thicken_fixture(ctx: &DecodeContext<'_>) -> (crate::feature::FeatureInventory, Feature) {
    let (inventory, mut feature) = thicken_fixture(ctx);
    retarget(
        &mut feature,
        "thicken",
        FeatureOperation::MoveFace {
            faces: FaceSelection::Native(inventory.properties[0].id()),
            motion: FaceMotion::Offset {
                distance: cadmpeg_ir::scalar::Length::new(2.0).expect("length"),
            },
        },
    );
    (inventory, feature)
}

#[test]
fn a_thicken_face_offset_joins_the_faces_of_every_wrapper_and_keeps_no_partial_selection() {
    let ctx = context();
    let (inventory, mut feature) = offset_thicken_fixture(&ctx);
    let sources = std::collections::HashSet::from([inventory.features[0].id()]);
    feature.native_ref = Some(inventory.features[0].id());
    assert_eq!(
        crate::feature::thicken_operand_losses(std::slice::from_ref(&feature), &sources).len(),
        1
    );
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    assert_eq!(states.len(), 1);
    let FaceSelection::Historical {
        faces: selected,
        native,
        ..
    } = faces(&feature)
    else {
        panic!("historical faces")
    };
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
    assert!(matches!(
        selection(&feature),
        Some((Kind::ThickenOffset, _))
    ));
    assert!(check(&inventory, &feature, &states));
    // The joined selection no longer counts as a native Thicken operand, and
    // it states the input-revision assumption like every joined face.
    assert!(
        crate::feature::thicken_operand_losses(std::slice::from_ref(&feature), &sources).is_empty()
    );
    assert_eq!(
        assumption_losses(std::slice::from_ref(&feature), &inventory).len(),
        1
    );
    // One unresolved face leaves the list native: only Move Face keeps a partial selection.
    let (mut inventory, mut feature) = offset_thicken_fixture(&ctx);
    inventory.edge_inputs.faces.remove(&(TOKEN.to_owned(), 43));
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
    // A repeated owner is a collision and leaves the list native.
    let (mut inventory, mut feature) = offset_thicken_fixture(&ctx);
    inventory
        .edge_inputs
        .faces
        .insert((TOKEN.to_owned(), 43), selected_face(5, 37, Surface::Other));
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

#[test]
fn a_thicken_tagged_translation_takes_no_face_join() {
    let ctx = context();
    let (inventory, mut feature) = thicken_fixture(&ctx);
    retarget(
        &mut feature,
        "thicken",
        FeatureOperation::MoveFace {
            faces: FaceSelection::Native(inventory.properties[0].id()),
            motion: FaceMotion::Translate {
                direction: cadmpeg_ir::features::FeatureDirection3::from(
                    cadmpeg_ir::units::UnitVector3::X_AXIS,
                ),
                distance: cadmpeg_ir::scalar::Length::new(2.0).expect("length"),
            },
        },
    );
    assert!(selection(&feature).is_none());
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
}

fn cone(index: usize, sine: f64, radius: f64) -> Record {
    record(
        index,
        "cone-surface",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Position([0.0, 0.0, 0.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([radius, 0.0, 0.0]),
            Token::Double(1.0),
            Token::False,
            Token::False,
            Token::Double(sine),
            Token::Double(1.0),
            Token::Double(radius),
        ],
    )
}

fn plane(index: usize) -> Record {
    record(
        index,
        "plane-surface",
        vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Position([0.0, 0.0, 1.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Vector3([1.0, 0.0, 0.0]),
        ],
    )
}

/// A face whose surface is record `surface`; field 8 is its sense.
fn face_on(index: usize, surface: usize, reversed: bool) -> Record {
    let mut tokens = vec![Token::Ref(-1), Token::Long(-1)];
    tokens.extend(std::iter::repeat_n(Token::Ref(-1), 5));
    tokens.push(Token::Ref(i64::try_from(surface).expect("small index")));
    tokens.push(if reversed { Token::True } else { Token::False });
    record(index, "face", tokens)
}

#[test]
fn a_selected_face_reports_the_surface_of_its_owner() {
    for schema in [38, 44] {
        for (surface, expected) in [
            (
                cone(3, 0.0, 0.25),
                Surface::Cone {
                    ratio: 1.0,
                    sine: 0.0,
                    radius: 0.25,
                },
            ),
            (plane(3), Surface::Plane),
            (
                record(3, "torus-surface", vec![Token::Ref(-1)]),
                Surface::Other,
            ),
        ] {
            let mut records = archive(schema);
            records[0] = face_on(1, 3, false);
            records.push(surface);
            assert_eq!(run(&records, schema).expect("selected").surface, expected);
        }
        // A cone with a non-unit axis ratio is not classified.
        let mut records = archive(schema);
        records[0] = face_on(1, 3, false);
        let mut elliptic = cone(3, 0.0, 0.25);
        let mut tokens = elliptic.tokens.to_vec();
        tokens[6] = Token::Double(0.5);
        elliptic.tokens = tokens.into();
        records.push(elliptic);
        assert_eq!(
            run(&records, schema).expect("selected").surface,
            Surface::Other
        );
    }
}

/// Two faces on separate surface records with matched attributes of one pair.
fn two_owners(schema: u32, second: Record, reversed: bool) -> Vec<Record> {
    let mut records = archive(schema);
    records[0] = face_on(1, 3, false);
    records.push(cone(3, 0.0, 0.25));
    records.push(face_on(2, 4, reversed));
    records.push(second);
    records.push(matched(12, 2, schema, 9, 7, 1));
    records
}

#[test]
fn several_owners_of_identical_surface_geometry_select_the_lowest_entity_index() {
    for schema in [38, 44] {
        let face = run(&two_owners(schema, cone(4, 0.0, 0.25), false), schema).expect("selected");
        assert_eq!(face.owner, 1);
        assert!(face.identical_owners);
        // Record order does not choose the owner.
        let mut reordered = two_owners(schema, cone(4, 0.0, 0.25), false);
        reordered.reverse();
        let face = run(&reordered, schema).expect("selected");
        assert_eq!(face.owner, 1);
        // A single owner states no such assumption.
        assert!(
            !run(&archive(schema), schema)
                .expect("selected")
                .identical_owners
        );
    }
}

#[test]
fn several_owners_of_different_surfaces_senses_or_unstored_geometry_select_nothing() {
    for schema in [38, 44] {
        for (name, records) in [
            ("radius", two_owners(schema, cone(4, 0.0, 0.5), false)),
            ("surface kind", two_owners(schema, plane(4), false)),
            ("sense", two_owners(schema, cone(4, 0.0, 0.25), true)),
        ] {
            assert!(run(&records, schema).is_none(), "{name} at schema {schema}");
        }
        // The second owner stores no surface.
        let mut unstored = two_owners(schema, cone(4, 0.0, 0.25), false);
        unstored[4] = face(2);
        assert!(run(&unstored, schema).is_none());
        // One owner of another qualifier.
        let mut mixed = two_owners(schema, cone(4, 0.0, 0.25), false);
        mixed.pop();
        mixed.push(matched(12, 2, schema, 9, 7, 0));
        assert!(run(&mixed, schema).is_none());
        // The second owner has no sense field.
        let mut senseless = two_owners(schema, cone(4, 0.0, 0.25), false);
        let mut tokens = senseless[4].tokens.to_vec();
        tokens.pop();
        senseless[4].tokens = tokens.into();
        assert!(run(&senseless, schema).is_none());
        // Both owners name a record that is not a surface.
        let mut unsurfaced = two_owners(schema, cone(4, 0.0, 0.25), false);
        for index in [3, 5] {
            let copy = unsurfaced[index].clone();
            let mut tokens = copy.tokens.to_vec();
            tokens[0] = Token::Ident("edge".into());
            unsurfaced[index] = record(copy.index, "edge", tokens);
        }
        assert!(run(&unsurfaced, schema).is_none());
        // One owner that is not a face.
        let mut edge = two_owners(schema, cone(4, 0.0, 0.25), false);
        edge[4] = record(2, "edge", vec![Token::Ref(-1)]);
        assert!(run(&edge, schema).is_none());
    }
}

#[test]
fn assumption_losses_state_identical_owners_for_every_face_selecting_kind() {
    let ctx = context();
    let (mut inventory, mut feature) = thread_fixture(
        &ctx,
        Surface::Cone {
            ratio: 1.0,
            sine: 0.0,
            radius: 0.25,
        },
        Some(5.0),
    );
    inventory
        .edge_inputs
        .faces
        .get_mut(&(TOKEN.to_owned(), 41))
        .expect("face")
        .identical_owners = true;
    assert!(assumption_losses(std::slice::from_ref(&feature), &inventory).is_empty());
    project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    let notes = assumption_losses(std::slice::from_ref(&feature), &inventory);
    assert_eq!(notes.len(), 2);
    assert!(notes[1].message.contains("identical surface geometry"));
}

/// Fields of a synthetic direct face selector.
struct Direct {
    state: u32,
    form: u8,
    topology: u32,
    qualifier: u32,
    naming: u32,
    generators: Vec<u32>,
    secondary: Vec<u32>,
    suffix: u8,
}

impl Direct {
    fn new() -> Self {
        Self {
            state: u32::MAX,
            form: 5,
            topology: 2,
            qualifier: 0,
            naming: 2,
            generators: vec![31, 37],
            secondary: vec![3],
            suffix: 0,
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
        if !(19..=21).contains(&version) {
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
        for word in [0x8000_0008_u32, 0x8000_0004, SELECTOR_KEY] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

fn parse_direct_bytes(bytes: &[u8], version: u8) -> Result<Option<FaceSelector>, CodecError> {
    parse_direct(&context(), View::over_retained(bytes), version)
}

#[test]
fn a_direct_face_selector_has_the_type_and_field_order_of_the_direct_edge_selector() {
    assert_eq!(
        DIRECT_SELECTOR_TYPE,
        [
            0x55, 0x4d, 0x87, 0x90, 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc, 0x06, 0x63,
            0xdc, 0x09
        ]
    );
    assert_ne!(DIRECT_SELECTOR_TYPE, SELECTOR_TYPE);
    for version in [19, 20, 21, 23, 29] {
        let bytes = Direct::new().bytes(version);
        let selector = parse_direct_bytes(&bytes, version)
            .expect("framed")
            .expect("direct selector");
        assert_eq!(selector.source, SELECTOR_SOURCE);
        assert_eq!(selector.key, SELECTOR_KEY);
        assert_eq!(selector.generators, [31, 37]);
        assert_eq!(selector.item.record_ordinal(), Some(7));
        assert_eq!(selector.body.record_ordinal(), Some(9));
        assert_eq!(selector.context.record_ordinal(), Some(3));
        let context_of = |ordinal| match ordinal {
            9 => Some(BODY_TYPE),
            7 => Some(crate::feature::FACE_ITEM_TYPE),
            3 => Some(CONTEXT_TYPE),
            _ => None,
        };
        assert!(links(&selector, context_of, |ordinal| (ordinal == 9).then_some(2)));
        // An edge item in the item slot is no face item.
        let edge_item = |ordinal| match ordinal {
            7 => Some(crate::feature::EDGE_ITEM_TYPE),
            other => context_of(other),
        };
        assert!(!links(&selector, edge_item, |ordinal| (ordinal == 9).then_some(2)));
        for count in 1..=4_u32 {
            let mut fields = Direct::new();
            fields.generators = (1..=count).map(|value| value * 10).collect();
            let parsed = parse_direct_bytes(&fields.bytes(version), version).expect("framed");
            assert_eq!(
                parsed.expect("generators").generators.len(),
                usize::try_from(count).expect("count")
            );
        }
        let mut empty = Direct::new();
        empty.secondary.clear();
        assert!(parse_direct_bytes(&empty.bytes(version), version)
            .expect("framed")
            .is_some());
    }
}

#[test]
fn a_direct_face_selector_that_differs_in_any_control_stays_unparsed() {
    type Edit = fn(&mut Direct);
    let edits: [(&str, Edit); 11] = [
        ("state is zero", |f| f.state = 0),
        ("form", |f| f.form = 4),
        ("topology and naming words differ", |f| f.naming = 3),
        ("qualifier", |f| f.qualifier = 1),
        ("no generator", |f| f.generators.clear()),
        ("five generators", |f| f.generators = vec![1, 2, 3, 4, 5]),
        ("zero generator", |f| f.generators = vec![31, 0]),
        ("repeated generator", |f| f.generators = vec![31, 31]),
        ("zero secondary name", |f| f.secondary = vec![0]),
        ("secondary name equals a generator", |f| {
            f.secondary = vec![31];
        }),
        ("suffix", |f| f.suffix = 1),
    ];
    for version in [19, 23, 29] {
        for (label, edit) in edits {
            let mut fields = Direct::new();
            edit(&mut fields);
            // A qualifier word exists only outside versions 19 to 21.
            if label == "qualifier" && (19..=21).contains(&version) {
                continue;
            }
            assert!(
                parse_direct_bytes(&fields.bytes(version), version)
                    .expect("framed refusal")
                    .is_none(),
                "{label} in version {version}"
            );
        }
        let bytes = Direct::new().bytes(version);
        assert!(parse_direct_bytes(&bytes[..bytes.len() - 1], version).is_err());
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(parse_direct_bytes(&extra, version).is_err());
    }
}

/// A feature-tag attribute of a schema-38, schema-40, or schema-42 carrier.
fn feature_tag(
    index: usize,
    owner: i64,
    schema: u32,
    family: &str,
    pairs: &[(i64, i64)],
) -> Record {
    let mut tokens = common(owner);
    tokens.push(Token::Long(i64::from(schema)));
    tokens.push(Token::Long(i64::try_from(pairs.len()).expect("small")));
    for (source, name) in pairs {
        tokens.extend([Token::Long(*source), Token::Long(*name)]);
    }
    record(
        index,
        &format!("{family}-NMx_Brep_tag-NamingMatching-attrib"),
        tokens,
    )
}

fn state_two_selector() -> FaceSelector {
    let mut fields = Fields::new();
    fields.state = 2;
    parse_bytes(&fields.bytes(23), 23)
        .expect("framed")
        .expect("state-2 face selector")
}

#[test]
fn a_face_selector_stores_state_zero_or_two_and_no_other_state() {
    for version in [19, 23, 29] {
        for (state, expected) in [(0_u32, Some(0)), (2, Some(2)), (1, None), (3, None)] {
            let mut fields = Fields::new();
            fields.state = state;
            let parsed = parse_bytes(&fields.bytes(version), version).expect("framed");
            assert_eq!(parsed.map(|selector| selector.state), expected, "{state}");
        }
    }
}

#[test]
fn the_feature_tags_of_the_owner_state_their_source_indexes() {
    for schema in [38, 40, 42, 43] {
        for family in ["NMx_Brep_Feature_tag", "NMx_tweak_reblend_tag"] {
            let mut records = archive(schema);
            records.push(feature_tag(12, 1, schema, family, &[(17, -1), (19, -1)]));
            records.push(feature_tag(
                13,
                1,
                schema,
                "NMx_tweak_reblend_tag",
                &[(5, -1)],
            ));
            let face = selected(&records, &[state_two_selector()], schema)
                .remove(&SELECTOR_SOURCE)
                .expect("selected face");
            assert_eq!(face.state, 2);
            assert_eq!(face.feature_sources, [5, 17, 19], "{family}");
        }
    }
    // Another family, another owner, a wrong schema, and a short pair vector
    // state no source.
    let schema = 42;
    let mut records = archive(schema);
    records.push(feature_tag(
        12,
        1,
        schema,
        "NMx_Import_Brep_tag",
        &[(17, -1)],
    ));
    records.push(feature_tag(
        13,
        2,
        schema,
        "NMx_Brep_Feature_tag",
        &[(17, -1)],
    ));
    records.push(feature_tag(14, 1, 40, "NMx_Brep_Feature_tag", &[(17, -1)]));
    let mut short = feature_tag(15, 1, schema, "NMx_Brep_Feature_tag", &[(17, -1)]);
    short.tokens = short.tokens[..short.tokens.len() - 1].into();
    records.push(short);
    let face = selected(&records, &[state_two_selector()], schema)
        .remove(&SELECTOR_SOURCE)
        .expect("selected face");
    assert!(face.feature_sources.is_empty());
    // A schema-44 carrier stores no legacy feature tag.
    let mut records = archive(44);
    records.push(feature_tag(12, 1, 44, "NMx_Brep_Feature_tag", &[(17, -1)]));
    let face = selected(&records, &[state_two_selector()], 44)
        .remove(&SELECTOR_SOURCE)
        .expect("selected face");
    assert!(face.feature_sources.is_empty());
}

#[test]
fn a_state_two_selector_joins_a_thicken_operand_only_through_its_own_feature_tag() {
    let ctx = context();
    // The test feature has source index 0.
    let own = 0;
    let admit = |edit: &dyn Fn(&mut SelectedFace)| {
        let (mut inventory, mut feature) = thicken_fixture(&ctx);
        for face in inventory.edge_inputs.faces.values_mut() {
            face.state = 2;
            face.feature_sources = vec![own];
            edit(face);
        }
        !project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    };
    assert!(admit(&|_| {}));
    // The tag of another feature, no tag, several owners, a repeated
    // attribute, and a state other than 0 and 2 leave the list native.
    assert!(!admit(&|face| face.feature_sources = vec![own + 1]));
    assert!(!admit(&|face| face.feature_sources.clear()));
    assert!(!admit(&|face| face.owners.push(face.owner + 10)));
    assert!(!admit(&|face| face.repeated_attribute = true));
    assert!(!admit(&|face| face.state = 1));
    // One state-2 operand among state-0 operands joins, and one refused
    // operand refuses the whole list.
    let (mut inventory, mut feature) = thicken_fixture(&ctx);
    let face = inventory
        .edge_inputs
        .faces
        .get_mut(&(TOKEN.to_owned(), 41))
        .expect("face");
    face.state = 2;
    face.feature_sources = vec![own];
    assert!(
        !project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    let (mut inventory, mut feature) = thicken_fixture(&ctx);
    inventory
        .edge_inputs
        .faces
        .get_mut(&(TOKEN.to_owned(), 41))
        .expect("face")
        .state = 2;
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

#[test]
fn a_state_two_selector_requires_an_owned_move_face_or_thicken_operand() {
    let face = |state| SelectedFace {
        state,
        feature_sources: vec![4],
        ..selected_face(5, 6, Surface::Other)
    };
    for kind in [
        Kind::MoveFace,
        Kind::Shell,
        Kind::Thread,
        Kind::Thicken,
        Kind::ThickenOffset,
    ] {
        assert!(state_admitted(kind, &face(0), 4), "{kind:?}");
        assert_eq!(
            state_admitted(kind, &face(2), 4),
            matches!(kind, Kind::MoveFace | Kind::Thicken | Kind::ThickenOffset)
        );
        assert!(!state_admitted(kind, &face(2), 5), "{kind:?}");
        assert!(!state_admitted(kind, &face(-1), 4), "{kind:?}");
    }
}

#[test]
fn modified_move_face_and_offset_operands_require_the_selecting_feature_tag() {
    let ctx = context();
    for offset in [false, true] {
        for admitted in [true, false] {
            let (mut inventory, mut feature) = if offset {
                offset_thicken_fixture(&ctx)
            } else {
                fixture(&ctx, [&[41], &[43]])
            };
            let own = inventory.features[0].header.source_index;
            for face in inventory.edge_inputs.faces.values_mut() {
                face.state = 2;
                face.feature_sources = vec![own + u32::from(!admitted)];
            }
            let states =
                project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
            assert_eq!(states.len(), usize::from(admitted));
            if admitted {
                assert_eq!(states[0].faces.len(), 2);
                assert!(check(&inventory, &feature, &states));
            } else {
                assert!(matches!(faces(&feature), FaceSelection::Native(_)));
            }
        }
    }
}

#[test]
fn modified_untagged_operands_keep_move_face_partial_and_offset_native() {
    let ctx = context();
    for offset in [false, true] {
        let (mut inventory, mut feature) = if offset {
            offset_thicken_fixture(&ctx)
        } else {
            fixture(&ctx, [&[41], &[43]])
        };
        let own = inventory.features[0].header.source_index;
        for ((_, selector), face) in &mut inventory.edge_inputs.faces {
            face.state = 2;
            face.feature_sources = if *selector == 41 {
                vec![own]
            } else {
                Vec::new()
            };
        }
        let states =
            project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
        if offset {
            assert!(states.is_empty());
            assert!(matches!(faces(&feature), FaceSelection::Native(_)));
        } else {
            assert_eq!(states.len(), 1);
            assert_eq!(states[0].faces.len(), 1);
            assert!(matches!(
                faces(&feature),
                FaceSelection::HistoricalPartial { .. }
            ));
            assert!(check(&inventory, &feature, &states));
        }
    }
}

#[test]
fn a_modified_schema_43_move_face_requires_its_input_revision_rule() {
    let ctx = context();
    let (mut inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    let own = inventory.features[0].header.source_index;
    for face in inventory.edge_inputs.faces.values_mut() {
        face.schema = 43;
        face.state = 2;
        face.feature_sources = vec![own];
    }
    assert!(
        project(&ctx, std::slice::from_mut(&mut feature), &inventory)
            .expect("project")
            .is_empty()
    );
    assert!(matches!(faces(&feature), FaceSelection::Native(_)));
}

#[test]
fn reader_and_validator_admit_the_same_face_carrier_schemas() {
    let ctx = context();
    let (inventory, mut feature) = fixture(&ctx, [&[41], &[43]]);
    let states = project(&ctx, std::slice::from_mut(&mut feature), &inventory).expect("project");
    for schema in [38, 40, 42, 43, 44] {
        assert!(run(&archive(schema), schema).is_some());
        assert!(check_with_schema(&inventory, &feature, &states, schema));
    }
    assert!(run(&archive(45), 45).is_none());
    assert!(!check_with_schema(&inventory, &feature, &states, 45));
}
