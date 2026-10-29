// SPDX-License-Identifier: Apache-2.0
//! Delivered classes are independent of decode requests and geometry failure.

use std::io::Cursor;

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::{ArenaClass, ContentScope};

use crate::test_support::container::{make_block, outer_header};
use crate::SldprtCodec;

fn design_without_geometry() -> Vec<u8> {
    let mut source = outer_header();
    source.extend(make_block(0x43, "Contents/Keywords", br#"<Keywords><Feature Name="Boss-Extrude1" Type="Extrusion" id="10"><Dimension Name="D1">10mm</Dimension></Feature><CustomProperty Name="PartNumber">A-123</CustomProperty></Keywords>"#));
    source
}

#[test]
fn metadata_fallback_holds_design_without_claiming_model() {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(design_without_geometry()),
            &DecodeOptions::default(),
        )
        .expect("metadata and design remain available");
    let (ir, report, fidelity) = decoded.into_parts();
    let ir = &ir;
    assert_eq!(
        ir.scope,
        ContentScope {
            model: false,
            design: true
        }
    );
    assert_eq!(report.scope(), ir.scope);
    assert!(!report.geometry_transferred());
    assert!(ir.model.populated_arena(ArenaClass::Model).is_none());
    assert!(!ir.model.features.is_empty());
    assert!(!ir.model.parameters.is_empty());
    assert!(!fidelity.unrepresented().contains(&ArenaClass::Model));
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code.local_code() == "depth.not-delivered"
            && loss.message.contains("model")));
    let namespace = ir
        .native
        .namespace("sldprt")
        .expect("typed native metadata");
    let native =
        crate::native::SldprtNative::load(namespace).expect("metadata passes typed admission");
    assert!(native
        .document_metadata
        .iter()
        .flat_map(|record| &record.attributes)
        .any(|attribute| attribute.name == "PartNumber"));
    let encoded = serde_json::to_vec(ir).expect("design-only CADIR serializes");
    let parsed: cadmpeg_ir::CadIr =
        serde_json::from_slice(&encoded).expect("design-only CADIR parses");
    assert_eq!(parsed.scope, ir.scope);
}

#[test]
fn metadata_without_design_delivers_empty_scope() {
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(outer_header()), &DecodeOptions::default())
        .expect("empty container remains a document");
    assert_eq!(decoded.ir().scope, ContentScope::container());
    assert_eq!(decoded.ir().model.entity_count(), 0);
    assert_eq!(decoded.report().scope(), ContentScope::container());
}

#[test]
fn explicit_container_request_delivers_no_neutral_arenas() {
    let options = DecodeOptions {
        depth: DecodeDepth::Container,
        ..DecodeOptions::default()
    };
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(design_without_geometry()), &options)
        .expect("container projection retains native records");
    assert_eq!(decoded.ir().scope, ContentScope::container());
    assert_eq!(decoded.ir().model.entity_count(), 0);
    assert_eq!(decoded.report().scope(), ContentScope::container());
    assert!(crate::native::SldprtNative::load(
        decoded
            .ir()
            .native
            .namespace("sldprt")
            .expect("native records")
    )
    .expect("typed records")
    .feature_histories
    .iter()
    .any(|history| !history.features.is_empty()));
}

#[test]
fn active_configuration_rebuild_flag_marks_current_features_stale() {
    let mut source = design_without_geometry();
    source.extend(make_block(
        0x43,
        "Contents/Keywords-configuration",
        br#"<Keywords><Configuration Name="Default" id="0"/></Keywords>"#,
    ));
    source.extend(make_block(0x43, "Contents/SolidWorks", br#"<swSolidWorks swVersion="34000"><swModel swConfigurationName="Default"/><swConfigurationList><swConfiguration swID="0" swConfigurationName="Default" swMostRecentConfiguration="YES" swConfigurationNeedsUpdate="YES"/></swConfigurationList></swSolidWorks>"#));
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("stale cached source still decodes");
    assert!(!decoded.ir().model.features.is_empty());
    assert!(decoded
        .ir()
        .model
        .features
        .iter()
        .all(|feature| feature.needs_recompute));
    let active = decoded
        .ir()
        .model
        .configurations
        .iter()
        .find(|configuration| configuration.active)
        .expect("manifest selects the current configuration");
    assert!(active.feature_states.is_empty());
    assert!(active.parameter_values.is_empty());
    assert!(active.parameter_overrides.is_empty());
}
