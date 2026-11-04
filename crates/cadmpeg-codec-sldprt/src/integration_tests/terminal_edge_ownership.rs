use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};

use crate::resolved_features::selections::COMPACT_EDGE_VECTOR_MARKER;
use crate::test_support::container::{make_block, sldprt_with_body};
use crate::test_support::history::resolved_feature_classes_with_ids;
use crate::test_support::native::sldprt_native;
use crate::test_support::parasolid::triangle_body;

fn selection_vector(terminal: u32) -> Vec<u8> {
    let mut payload = vec![0; 160];
    payload[..4].copy_from_slice(&4u32.to_le_bytes());
    payload[4..8].copy_from_slice(&[0, 2, 0, 0]);
    payload[12..28].copy_from_slice(&COMPACT_EDGE_VECTOR_MARKER);
    let mut cursor = 30;
    for (index, local) in [32u32, 34, 1].into_iter().enumerate() {
        payload[cursor..cursor + 4].copy_from_slice(&[0x3d, 0x80, 0, 0]);
        payload[cursor + 4..cursor + 16]
            .copy_from_slice(&[0x34, 0x80, 0x37, 0, 75, 0, 0, 0, 0x9b, 0x95, 0x90, 0x5f]);
        payload[cursor + 16..cursor + 20].copy_from_slice(&local.to_le_bytes());
        cursor += 20;
        if index != 2 {
            payload[cursor..cursor + 8].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]);
            cursor += 8;
        }
    }
    payload[cursor..cursor + 36].copy_from_slice(&[
        1, 0, 0, 0, 0, 0, 0, 0, 0x4a, 0x80, 0, 0, 0x34, 0x80, 0x37, 0, 0, 0, 0, 0, 0x89, 0x6b,
        0x90, 0x5f, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    payload[cursor + 16..cursor + 20].copy_from_slice(&terminal.to_le_bytes());
    payload
}

fn lane_payload(terminal: u32) -> Vec<u8> {
    let mut payload = resolved_feature_classes_with_ids(&[("Fillet_c", "Round", 92)]);
    payload.extend([0; 16]);
    payload.extend(selection_vector(terminal));
    payload
}

fn source(terminals: &[u32]) -> Vec<u8> {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords><Configuration Name="Default" SourceIndex="0"/><Configuration Name="Alternate" SourceIndex="1"/><Feature Name="Old" Type="Imported" id="75"/><Feature Name="Round" Type="Fillet" id="92"><Dimension Name="Radius">1mm</Dimension></Feature><Feature Name="Later" Type="Imported" id="114"/></Keywords>"#,
    ));
    for (index, terminal) in terminals.iter().copied().enumerate() {
        source.extend(make_block(
            0x42,
            &format!("Contents/Config-{index}-ResolvedFeatures"),
            &lane_payload(terminal),
        ));
    }
    source
}

fn edges(definition: &FeatureDefinition) -> &EdgeSelection {
    match definition {
        FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) => &groups[0].edges,
        other => panic!("expected authored fillet, got {other:?}"),
    }
}

#[test]
fn terminal_owner_refusal_survives_native_store_projection_and_loss_reporting() {
    for (terminal, refused) in [(75, false), (92, true), (114, true), (999, true)] {
        let result = super::decode(source(&[terminal]));
        let native = sldprt_native(result.ir());
        let lane = native
            .feature_input_lanes
            .iter()
            .find(|lane| !lane.edge_selections.is_empty())
            .unwrap();
        assert_eq!(lane.edge_selections.len(), 1);
        let selection = &lane.edge_selections[0];
        assert_eq!(selection.terminal_feature_ref.is_none(), refused);
        assert_eq!(selection.producer_feature_refs.len(), 1);
        let consumer = result
            .ir()
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(selection.feature_ref.as_str()))
            .unwrap();
        if refused {
            assert!(matches!(
                edges(consumer.evaluation.definition()),
                EdgeSelection::Native(_)
            ));
        } else {
            assert!(matches!(
                edges(consumer.evaluation.definition()),
                EdgeSelection::Generated { .. }
            ));
        }
        let losses = result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code.local_code() == "feature.edge-terminal-unresolved")
            .collect::<Vec<_>>();
        assert_eq!(losses.len(), usize::from(refused));
        if let Some(loss) = losses.first() {
            assert_eq!(
                loss.message,
                "1 native edge selection(s) retain an unresolved terminal feature owner."
            );
        }
        let validation = crate::resolved_features::validate::validate_native(
            &cadmpeg_test_support::service_decode_context(),
            result.ir(),
        )
        .unwrap();
        assert!(validation.is_empty(), "{validation:?}");
    }
}

#[test]
fn refused_configuration_terminal_is_not_refilled_from_valid_base_lane() {
    let result = super::decode(source(&[75, 92]));
    let native = sldprt_native(result.ir());
    let selections = native
        .feature_input_lanes
        .iter()
        .flat_map(|lane| &lane.edge_selections)
        .collect::<Vec<_>>();
    assert_eq!(selections.len(), 2);
    let consumer = result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(selections[0].feature_ref.as_str()))
        .unwrap();
    for (source_index, refused) in [(0, false), (1, true)] {
        let configuration = result
            .ir()
            .model
            .configurations
            .iter()
            .find(|configuration| configuration.source_index == Some(source_index))
            .unwrap();
        let state = &configuration.feature_states[&consumer.id];
        if refused {
            assert!(matches!(edges(&state.definition), EdgeSelection::Native(_)));
        } else {
            assert!(matches!(
                edges(&state.definition),
                EdgeSelection::Generated { .. }
            ));
        }
    }
    let losses = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.local_code() == "feature.edge-terminal-unresolved")
        .collect::<Vec<_>>();
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].message,
        "1 native edge selection(s) retain an unresolved terminal feature owner."
    );
}
