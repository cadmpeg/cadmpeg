// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::products::ProductDefinitionKind;
use cadmpeg_ir::{Codec, DecodeOptions};
use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::container::{
    make_block, outer_header, sldprt_with_body, synthetic_sldprt,
};
use crate::test_support::history::sldprt_with_body_and_history;
use crate::test_support::parasolid::triangle_body;
use crate::SldprtCodec;

const PART_ROOT: &str = "sldprt:model:product-definition#root";
const ASSEMBLY_ROOT: &str = "sldprt:assembly:model#2";
const TREE: &[u8] = br#"<?xml version="1.0"?>
<swSolidWorks swVersion="17000">
 <swHeader>
  <swFile id="3" swDocType="ASSEMBLY" swPath="C:\work\Top.SLDASM"/>
  <swFile id="6" swDocType="ASSEMBLY" swPath="C:\work\Sub.SLDASM"/>
  <swFile id="9" swDocType="PART" swPath="C:\work\Pin.SLDPRT"/>
  <swFile id="12" swDocType="PART" swPath="Box^Top.sldprt"/>
 </swHeader>
 <swModelList>
  <swModel id="8" swName="Pin" swConfigurationName="Default" swFileRef="9"/>
  <swModel id="11" swName="Box" swConfigurationName="Default" swFileRef="12"/>
  <swModel id="5" swName="Sub" swConfigurationName="Default" swFileRef="6">
   <swReference id="14" swName="Pin" swModelRef="8" swTransform="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/>
  </swModel>
  <swModel id="2" swName="Top" swConfigurationName="Default" swFileRef="3">
   <swReference id="4" swName="Sub" swModelRef="5" swTransform="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/>
   <swReference id="7" swName="Box" swModelRef="11" swIsVirtualComponent="YES" swTransform="1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"/>
  </swModel>
 </swModelList>
 <swConfigurationList>
  <swConfiguration id="1" swName="Default" swMostRecentConfiguration="YES" swModelRef="2"/>
 </swConfigurationList>
</swSolidWorks>"#;

fn decode(source: &[u8]) -> EditableDecodeResult {
    EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
            .expect("decode synthetic document"),
    )
}

fn assert_root(decoded: &EditableDecodeResult, id: &str) {
    let roots = decoded
        .ir()
        .model
        .product_definitions
        .iter()
        .filter(|definition| definition.is_root())
        .collect::<Vec<_>>();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].id.as_str(), id);
    let validation = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .expect("validate synthetic document");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
    assert!(!decoded
        .source_fidelity()
        .edited(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            decoded.ir(),
        )
        .expect("final document digest"));
}

#[test]
fn assembly_decode_states_one_root_with_local_and_virtual_body_owners() {
    let mut source = sldprt_with_body_and_history(&triangle_body());
    source.extend(make_block(0x42, "swXmlContents/COMPINSTANCETREE", TREE));
    source.extend(make_block(
        0x42,
        "VirtualComp/Box-sldprt",
        &sldprt_with_body(&triangle_body()),
    ));
    let decoded = decode(&source);
    assert_root(&decoded, ASSEMBLY_ROOT);
    let ir = decoded.ir();
    let root = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.is_root())
        .expect("assembly root");
    assert_eq!(root.kind, Some(ProductDefinitionKind::Assembly));
    assert_eq!(root.bodies.len(), 1);
    assert_eq!(root.occurrences.len(), 2);
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_eq!(ir.model.occurrences.len(), 3);
    let virtual_part = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.id.as_str() == "sldprt:assembly:model#11")
        .expect("virtual part definition");
    assert_eq!(virtual_part.bodies.len(), 1);
    assert_ne!(root.bodies, virtual_part.bodies);
    for body in &ir.geometry.bodies {
        assert_eq!(
            ir.model
                .product_definitions
                .iter()
                .filter(|definition| definition.bodies.contains(&body.id))
                .count(),
            1
        );
    }
    assert!(ir.design.scopes.is_empty());
    assert!(!json::to_string(ir)
        .expect("serialize IR references")
        .contains(PART_ROOT));
    assert!(!json::to_string(decoded.source_fidelity())
        .expect("serialize source fidelity references")
        .contains(PART_ROOT));
    assert_eq!(
        decoded.source_fidelity().decode_digest,
        Sha256Digest::digest(&source)
    );
}

#[test]
fn metadata_assembly_decode_states_the_assembly_root() {
    let mut source = outer_header();
    source.extend(make_block(0x42, "swXmlContents/COMPINSTANCETREE", TREE));
    let decoded = decode(&source);
    assert_root(&decoded, ASSEMBLY_ROOT);
    assert!(decoded.ir().geometry.bodies.is_empty());
}

#[test]
fn part_decode_keeps_its_root_and_design_scope() {
    for source in [
        synthetic_sldprt(),
        sldprt_with_body(&triangle_body()),
        sldprt_with_body_and_history(&triangle_body()),
    ] {
        let decoded = decode(&source);
        assert_root(&decoded, PART_ROOT);
        let ir = decoded.ir();
        let root = &ir.model.product_definitions[0];
        assert_eq!(
            root.bodies,
            ir.geometry
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
        );
        assert!(ir
            .design
            .scopes
            .iter()
            .all(|scope| scope.definition == root.id));
    }
    let with_history = decode(&sldprt_with_body_and_history(&triangle_body()));
    assert_eq!(with_history.ir().design.scopes.len(), 1);
}
