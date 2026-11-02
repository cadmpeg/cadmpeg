// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::products::{ExternalDocument, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::StreamName;

use super::{placement, project};
use crate::container::{Block, BlockName, CompoundStream, ContainerScan, PayloadFamily};
use crate::loss::SldprtLossCode;

/// Rotation of +90 degrees about z, written for row vectors as `SolidWorks`
/// stores it, with a translation of 20 mm along y.
const ROTATED: &str = "0 1 0 0 -1 0 0 0 0 0 1 0 0 0.02 0 1";
const SHIFTED_X: &str = "1 0 0 0 0 1 0 0 0 0 1 0 0.003 0 0 1";
const SHIFTED_X_10: &str = "1 0 0 0 0 1 0 0 0 0 1 0 0.01 0 0 1";
const IDENTITY: &str = "1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1";

/// A top assembly (model 2) that places sub-assembly model 5 twice, a
/// suppressed part, and a hidden virtual part. Model 5 places part model 8 in
/// its `Long` configuration. Model 15 is an inactive flexible configuration of
/// the top assembly and is not walked.
fn tree() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<swSolidWorks xmlns="http://www.solidworks.com/sw2003/schema" swVersion="17000">
 <swHeader>
  <swFile id="3" swDocType="ASSEMBLY" swPath="C:\work\Top.SLDASM"/>
  <swFile id="6" swDocType="ASSEMBLY" swPath="C:\work\Sub.SLDASM"/>
  <swFile id="9" swDocType="PART" swPath="C:\work\Pin.SLDPRT"/>
  <swFile id="12" swDocType="PART" swPath="Box^Top.sldprt"/>
 </swHeader>
 <swModelList>
  <swModel id="8" swName="Pin" swConfigurationName="Long" swFileRef="9"/>
  <swModel id="13" swName="Box" swConfigurationName="Default" swFileRef="12"/>
  <swModel id="5" swName="Top" swConfigurationName="Default" swFileRef="6">
   <swReference id="14" swName="Pin" swReferenceNumber="1" swModelRef="8" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="{SHIFTED_X}"/>
  </swModel>
  <swModel id="2" swName="Top" swConfigurationName="Default" swFileRef="3">
   <swReference id="4" swName="Sub" swReferenceNumber="1" swModelRef="5" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="{SHIFTED_X_10}"/>
   <swReference id="7" swName="Sub" swReferenceNumber="2" swModelRef="5" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="{ROTATED}"/>
   <swReference id="10" swName="Pin" swReferenceNumber="2" swModelRef="8" swSuppressed="YES" swHidden="NO" swIsVirtualComponent="NO" swTransform="{IDENTITY}"/>
   <swReference id="11" swName="Box" swReferenceNumber="1" swModelRef="13" swSuppressed="NO" swHidden="YES" swIsVirtualComponent="YES" swTransform="{IDENTITY}"/>
  </swModel>
  <swModel id="15" swName="Top" swConfigurationName="Default-_flexible1" swFileRef="3">
   <swReference id="16" swName="Pin" swReferenceNumber="3" swModelRef="8" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="{IDENTITY}"/>
  </swModel>
 </swModelList>
 <swConfigurationList>
  <swConfiguration id="1" swName="Default" swMostRecentConfiguration="YES" swModelRef="2"/>
  <swConfiguration id="17" swName="Default-_flexible1" swMostRecentConfiguration="NO" swModelRef="15"/>
 </swConfigurationList>
</swSolidWorks>"#
    )
}

fn ctx() -> cadmpeg_core::decode::DecodeContext<'static> {
    cadmpeg_test_support::service_decode_context()
}

fn named_block(section: &str, payload: Vec<u8>) -> Block {
    Block {
        offset: 0,
        type_id: 0x43,
        comp_sz: 0,
        section: BlockName::Named(StreamName::try_from(section.to_owned()).expect("a stream name")),
        family: PayloadFamily::Unknown,
        payload,
        ps_streams: Vec::new(),
        ps_probe_capped: false,
    }
}

fn synthetic_scan<'a>(
    blocks: Vec<Block>,
    compound_streams: Vec<CompoundStream<'a>>,
) -> ContainerScan<'a> {
    ContainerScan {
        source_image: &[],
        version: 0,
        blocks,
        directory: Vec::new(),
        cache_cells: Vec::new(),
        compound_streams,
        block_views: std::collections::BTreeMap::new(),
        compound_views: std::collections::BTreeMap::new(),
        solidworks: crate::container::SolidWorksEnvelopeScan::default(),
    }
}

fn assert_structure_loss(projection: &super::AssemblyProjection, message: &str) {
    assert_eq!(projection.losses().len(), 1);
    assert_eq!(
        projection.losses()[0].code,
        SldprtLossCode::AssemblyStructureNotTransferred.kind()
    );
    assert_eq!(projection.losses()[0].message, message);
}

fn occurrence<'p>(
    projection: &'p super::AssemblyProjection,
    id: &str,
) -> &'p cadmpeg_ir::products::Occurrence {
    projection
        .occurrences()
        .iter()
        .find(|occurrence| occurrence.id.as_str() == id)
        .unwrap_or_else(|| panic!("occurrence {id}"))
}

fn codes(projection: &super::AssemblyProjection) -> Vec<String> {
    projection
        .losses()
        .iter()
        .map(|loss| loss.code.local_code().to_string())
        .collect()
}

#[test]
fn a_sub_assembly_placed_twice_expands_under_each_instance() {
    let projection = project(&ctx(), &tree()).expect("project");
    let ids: Vec<&str> = projection
        .occurrences()
        .iter()
        .map(|occurrence| occurrence.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "sldprt:assembly:occurrence#root",
            "sldprt:assembly:occurrence#4",
            "sldprt:assembly:occurrence#4-14",
            "sldprt:assembly:occurrence#7",
            "sldprt:assembly:occurrence#7-14",
            "sldprt:assembly:occurrence#11",
        ]
    );
    let definitions: Vec<&str> = projection
        .definitions()
        .iter()
        .map(|definition| definition.id.as_str())
        .collect();
    assert_eq!(
        definitions,
        ["sldprt:assembly:model#2", "sldprt:assembly:model#5"]
    );
    assert_eq!(
        projection.definitions()[1].source_name.as_deref(),
        Some("Sub")
    );
    let nested = occurrence(&projection, "sldprt:assembly:occurrence#7-14");
    assert_eq!(
        nested.parent,
        OccurrenceParent::Occurrence {
            occurrence: occurrence(&projection, "sldprt:assembly:occurrence#7")
                .id
                .clone()
        }
    );
    assert_eq!(nested.name.as_deref(), Some("Pin-1"));
}

#[test]
fn a_placement_is_the_transposed_rotation_and_a_translation_in_millimetres() {
    let rows = placement(ROTATED).expect("placement").affine_rows();
    assert_eq!(
        rows,
        [
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    let projection = project(&ctx(), &tree()).expect("project");
    let nested = occurrence(&projection, "sldprt:assembly:occurrence#7-14");
    assert_eq!(
        nested.transform.map(cadmpeg_ir::transform::Transform::affine_rows),
        Some([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]),
        "a child's placement is relative to the assembly that contains it"
    );
}

#[test]
fn a_placement_with_a_projective_column_or_a_bad_count_is_refused() {
    assert!(placement("1 0 0 0.5 0 1 0 0 0 0 1 0 0 0 0 1").is_none());
    assert!(placement("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 2").is_none());
    assert!(placement("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0").is_none());
    assert!(placement("1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0").is_none());
    assert!(placement("1 0 0 0 0 1 0 0 0 0 1 0 NaN 0 0 1").is_none());
}

#[test]
fn a_leaf_part_is_an_external_document_in_its_configuration() {
    let projection = project(&ctx(), &tree()).expect("project");
    assert_eq!(
        occurrence(&projection, "sldprt:assembly:occurrence#4-14").prototype,
        PrototypeReference::External {
            document: ExternalDocument::path(r"C:\work\Pin.SLDPRT"),
            object: Some("Long".into()),
        }
    );
}

#[test]
fn suppressed_virtual_hidden_and_inactive_states_are_reported() {
    let projection = project(&ctx(), &tree()).expect("project");
    assert!(projection
        .occurrences()
        .iter()
        .all(|occurrence| occurrence.native_ref.as_deref() != Some("sldprt:tree:swReference#10")));
    assert!(projection
        .occurrences()
        .iter()
        .all(|occurrence| occurrence.native_ref.as_deref() != Some("sldprt:tree:swReference#16")));
    let virtual_part = occurrence(&projection, "sldprt:assembly:occurrence#11");
    assert_eq!(virtual_part.prototype, PrototypeReference::Unresolved {});
    assert_eq!(virtual_part.visible, Some(false));
    let mut expected = vec![
        SldprtLossCode::AssemblyConfigurationsNotTransferred,
        SldprtLossCode::AssemblyComponentsExternal,
        SldprtLossCode::AssemblyVirtualComponentsNotTransferred,
        SldprtLossCode::AssemblySuppressedComponentsOmitted,
    ]
    .into_iter()
    .map(|code| code.kind().local_code().to_string())
    .collect::<Vec<_>>();
    let mut found = codes(&projection);
    expected.sort();
    found.sort();
    assert_eq!(found, expected);
}

#[test]
fn a_tree_without_one_active_configuration_transfers_nothing() {
    let text = tree().replace(
        r#"swMostRecentConfiguration="YES""#,
        r#"swMostRecentConfiguration="NO""#,
    );
    let projection = project(&ctx(), &text).expect("project");
    assert!(projection.occurrences().is_empty());
    assert_eq!(
        codes(&projection),
        [SldprtLossCode::AssemblyStructureNotTransferred
            .kind()
            .local_code()
            .to_string()]
    );
}

#[test]
fn a_reference_to_an_undefined_model_is_omitted_and_reported() {
    let text = tree().replace(r#"swModelRef="5" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="0 1"#, r#"swModelRef="99" swSuppressed="NO" swHidden="NO" swIsVirtualComponent="NO" swTransform="0 1"#);
    let projection = project(&ctx(), &text).expect("project");
    assert!(projection
        .occurrences()
        .iter()
        .all(|occurrence| !occurrence.id.as_str().ends_with("#7")));
    assert!(codes(&projection).contains(
        &SldprtLossCode::AssemblyPlacementUnresolved
            .kind()
            .local_code()
            .to_string()
    ));
}

fn decoded_without_a_model() -> cadmpeg_ir::codec::Decoded {
    let mut body = cadmpeg_ir::codec::DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer {
        scope: cadmpeg_ir::document::ContentScope::container(),
        geometry_transferred: false,
    });
    for code in super::PART_MODEL_ABSENT {
        body.losses.push(code.note("no model"));
    }
    cadmpeg_ir::codec::Decoded {
        ir: cadmpeg_ir::document::CadIr::empty(),
        body,
        source_fidelity: cadmpeg_ir::SourceFidelity::with_annotations(
            cadmpeg_ir::Annotations::default(),
            cadmpeg_ir::hash::digest::Sha256Digest::digest(b"no-model"),
        ),
    }
}

fn remaining_part_losses(decoded: &cadmpeg_ir::codec::Decoded) -> usize {
    let absent = super::PART_MODEL_ABSENT.map(SldprtLossCode::kind);
    decoded
        .body
        .losses
        .iter()
        .filter(|loss| absent.contains(&loss.code))
        .count()
}

#[test]
fn an_assembly_without_its_own_parasolid_model_retires_the_part_model_losses() {
    let mut decoded = decoded_without_a_model();
    project(&ctx(), &tree())
        .expect("project")
        .apply(&mut decoded);
    assert_eq!(remaining_part_losses(&decoded), 0);
    assert_eq!(decoded.ir.model.occurrences.len(), 6);
}

#[test]
fn an_assembly_with_its_own_parasolid_stream_keeps_the_part_model_losses() {
    let mut decoded = decoded_without_a_model();
    let mut projection = project(&ctx(), &tree()).expect("project");
    projection.carries_parasolid = true;
    projection.apply(&mut decoded);
    assert_eq!(remaining_part_losses(&decoded), 3);
}

fn affine(rows: [[f64; 4]; 3]) -> Transform {
    Transform::affine(rows).expect("finite affine rows")
}

#[test]
fn the_component_tree_records_native_structure_and_projects_it() {
    let native = super::native_project(&ctx(), &tree()).expect("native project");
    let root_definition = cadmpeg_ir::ids::ProductDefinitionId::mint("sldprt:assembly:model#2")
        .expect("the root definition identity");
    let sub_definition = cadmpeg_ir::ids::ProductDefinitionId::mint("sldprt:assembly:model#5")
        .expect("the sub-assembly definition identity");
    let definitions = native.definitions();
    assert_eq!(definitions.len(), 2);
    assert_eq!(definitions[0].id(), &root_definition);
    assert_eq!(definitions[0].source_name(), Some("Top"));
    assert_eq!(definitions[0].native_ref(), "sldprt:tree:swModel#2");
    assert_eq!(definitions[1].id(), &sub_definition);
    assert_eq!(definitions[1].source_name(), Some("Sub"));
    assert_eq!(definitions[1].native_ref(), "sldprt:tree:swModel#5");

    let placements = native.placements();
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.id().as_str())
            .collect::<Vec<_>>(),
        [
            "sldprt:assembly:occurrence#root",
            "sldprt:assembly:occurrence#4",
            "sldprt:assembly:occurrence#4-14",
            "sldprt:assembly:occurrence#7",
            "sldprt:assembly:occurrence#7-14",
            "sldprt:assembly:occurrence#11",
        ]
    );
    assert_eq!(
        placements
            .iter()
            .map(super::native::NativePlacement::native_ref)
            .collect::<Vec<_>>(),
        [
            "sldprt:tree:swModel#2",
            "sldprt:tree:swReference#4",
            "sldprt:tree:swReference#14",
            "sldprt:tree:swReference#7",
            "sldprt:tree:swReference#14",
            "sldprt:tree:swReference#11",
        ]
    );
    assert!(matches!(
        placements[0].prototype(),
        super::native::NativePrototype::Local(definition) if definition == &root_definition
    ));
    assert_eq!(placements[0].parent(), &OccurrenceParent::Root {});
    assert_eq!(placements[0].ordinal(), 0);
    assert_eq!(placements[0].transform(), Transform::identity());
    assert_eq!(placements[0].name(), Some("Top"));
    assert_eq!(placements[0].visible(), None);
    assert!(matches!(
        placements[1].prototype(),
        super::native::NativePrototype::Local(definition) if definition == &sub_definition
    ));
    assert_eq!(
        placements[1].parent(),
        &OccurrenceParent::Occurrence {
            occurrence: placements[0].id().clone(),
        }
    );
    assert_eq!(
        placements[1].transform(),
        affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ])
    );
    assert_eq!(placements[1].name(), Some("Sub-1"));
    match placements[2].prototype() {
        super::native::NativePrototype::External { document, object } => {
            assert_eq!(document, r"C:\work\Pin.SLDPRT");
            assert_eq!(object.as_deref(), Some("Long"));
        }
        other => panic!("the pin is an external placement, got {other:?}"),
    }
    assert_eq!(
        placements[2].parent(),
        &OccurrenceParent::Occurrence {
            occurrence: placements[1].id().clone(),
        }
    );
    assert_eq!(placements[2].ordinal(), 0);
    assert_eq!(
        placements[2].transform(),
        affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ])
    );
    assert_eq!(placements[2].name(), Some("Pin-1"));
    assert_eq!(
        placements[3].transform(),
        affine([
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 0.0]
        ])
    );
    assert_eq!(placements[3].name(), Some("Sub-2"));
    assert_eq!(
        placements[4].parent(),
        &OccurrenceParent::Occurrence {
            occurrence: placements[3].id().clone(),
        }
    );
    assert_eq!(placements[4].name(), Some("Pin-1"));
    assert!(matches!(
        placements[5].prototype(),
        super::native::NativePrototype::Unresolved
    ));
    assert_eq!(placements[5].ordinal(), 3);
    assert_eq!(placements[5].transform(), Transform::identity());
    assert_eq!(placements[5].name(), Some("Box-1"));
    assert_eq!(placements[5].visible(), Some(false));

    let expected_losses = [
        "The component trees of 1 other configuration(s) are not transferred: \
         `Default-_flexible1`.",
        "2 component occurrence(s) reference 1 external document(s). The referenced documents \
         are not opened.",
        "1 occurrence(s) of virtual components have an unresolved prototype: `Box-1`.",
        "1 suppressed component instance(s) are omitted with the instances below them: `Pin-2`.",
    ];
    assert_eq!(
        native
            .losses()
            .iter()
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>(),
        expected_losses
    );
    assert_eq!(
        native
            .losses()
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [
            SldprtLossCode::AssemblyConfigurationsNotTransferred.kind(),
            SldprtLossCode::AssemblyComponentsExternal.kind(),
            SldprtLossCode::AssemblyVirtualComponentsNotTransferred.kind(),
            SldprtLossCode::AssemblySuppressedComponentsOmitted.kind(),
        ]
    );

    let projected = native.into_projection();
    let definitions = projected.definitions();
    assert_eq!(definitions.len(), 2);
    assert_eq!(definitions[0].id, root_definition.clone());
    assert!(matches!(
        definitions[0].kind,
        cadmpeg_ir::products::ProductDefinitionKind::Part
    ));
    assert_eq!(definitions[0].source_name.as_deref(), Some("Top"));
    assert_eq!(
        definitions[0].native_ref.as_deref(),
        Some("sldprt:tree:swModel#2")
    );
    assert!(definitions[0].bodies.is_empty());
    assert_eq!(definitions[1].id, sub_definition.clone());
    assert_eq!(definitions[1].source_name.as_deref(), Some("Sub"));
    assert_eq!(
        definitions[1].native_ref.as_deref(),
        Some("sldprt:tree:swModel#5")
    );

    let occurrences = projected.occurrences();
    assert_eq!(
        occurrences
            .iter()
            .map(|occurrence| occurrence.id.as_str())
            .collect::<Vec<_>>(),
        [
            "sldprt:assembly:occurrence#root",
            "sldprt:assembly:occurrence#4",
            "sldprt:assembly:occurrence#4-14",
            "sldprt:assembly:occurrence#7",
            "sldprt:assembly:occurrence#7-14",
            "sldprt:assembly:occurrence#11",
        ]
    );
    assert_eq!(
        occurrences[0].prototype,
        PrototypeReference::Local {
            definition: root_definition,
        }
    );
    assert_eq!(occurrences[0].parent, OccurrenceParent::Root {});
    assert_eq!(occurrences[0].ordinal, 0);
    assert_eq!(occurrences[0].transform, Some(Transform::identity()));
    assert_eq!(occurrences[0].name.as_deref(), Some("Top"));
    assert_eq!(occurrences[0].visible, None);
    assert_eq!(
        occurrences[0].native_ref.as_deref(),
        Some("sldprt:tree:swModel#2")
    );
    assert_eq!(
        occurrences[1].prototype,
        PrototypeReference::Local {
            definition: sub_definition,
        }
    );
    assert_eq!(
        occurrences[1].parent,
        OccurrenceParent::Occurrence {
            occurrence: occurrences[0].id.clone(),
        }
    );
    assert_eq!(
        occurrences[1].transform,
        Some(affine([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]))
    );
    assert_eq!(occurrences[1].name.as_deref(), Some("Sub-1"));
    assert_eq!(
        occurrences[1].native_ref.as_deref(),
        Some("sldprt:tree:swReference#4")
    );
    assert_eq!(
        occurrences[2].prototype,
        PrototypeReference::External {
            document: ExternalDocument::path(r"C:\work\Pin.SLDPRT"),
            object: Some("Long".into()),
        }
    );
    assert_eq!(
        occurrences[2].parent,
        OccurrenceParent::Occurrence {
            occurrence: occurrences[1].id.clone(),
        }
    );
    assert_eq!(occurrences[2].ordinal, 0);
    assert_eq!(
        occurrences[2].transform,
        Some(affine([
            [1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]))
    );
    assert_eq!(occurrences[2].name.as_deref(), Some("Pin-1"));
    assert_eq!(occurrences[2].visible, Some(true));
    assert_eq!(
        occurrences[2].native_ref.as_deref(),
        Some("sldprt:tree:swReference#14")
    );
    assert_eq!(occurrences[3].ordinal, 1);
    assert_eq!(
        occurrences[3].transform,
        Some(affine([
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 20.0],
            [0.0, 0.0, 1.0, 0.0]
        ]))
    );
    assert_eq!(occurrences[3].name.as_deref(), Some("Sub-2"));
    assert_eq!(
        occurrences[4].parent,
        OccurrenceParent::Occurrence {
            occurrence: occurrences[3].id.clone(),
        }
    );
    assert_eq!(occurrences[4].name.as_deref(), Some("Pin-1"));
    assert_eq!(occurrences[5].prototype, PrototypeReference::Unresolved {});
    assert_eq!(occurrences[5].ordinal, 3);
    assert_eq!(occurrences[5].name.as_deref(), Some("Box-1"));
    assert_eq!(occurrences[5].visible, Some(false));
    assert_eq!(occurrences[5].transform, Some(Transform::identity()));
    assert_eq!(
        occurrences[5].native_ref.as_deref(),
        Some("sldprt:tree:swReference#11")
    );
    assert_eq!(
        projected
            .losses()
            .iter()
            .map(|loss| loss.message.as_str())
            .collect::<Vec<_>>(),
        expected_losses
    );
}

#[test]
fn the_component_tree_structure_losses_state_the_exact_message() {
    let projection = project(&ctx(), "<notSolidWorks/>").expect("project");
    assert_structure_loss(
        &projection,
        "The `swXmlContents/COMPINSTANCETREE` section root is `notSolidWorks`. No occurrence \
         is transferred.",
    );

    let text = tree().replace(
        r#"swMostRecentConfiguration="YES""#,
        r#"swMostRecentConfiguration="NO""#,
    );
    let projection = project(&ctx(), &text).expect("project");
    assert_structure_loss(
        &projection,
        "The component tree names 0 active configurations. No occurrence is transferred.",
    );

    let text = tree().replace(
        r#"swMostRecentConfiguration="YES" swModelRef="2""#,
        r#"swMostRecentConfiguration="YES" swModelRef="99""#,
    );
    let projection = project(&ctx(), &text).expect("project");
    assert_structure_loss(
        &projection,
        "The active configuration names model `99`, which the component tree does not define. \
         No occurrence is transferred.",
    );
}

#[test]
fn a_document_without_a_component_tree_states_the_exact_structure_loss() {
    let scan = synthetic_scan(
        vec![named_block(super::TREE_SECTION, vec![0xff])],
        Vec::new(),
    );
    let projection = super::read(&ctx(), &scan)
        .expect("read")
        .expect("an assembly document");
    assert_structure_loss(
        &projection,
        "The assembly document has no readable `swXmlContents/COMPINSTANCETREE` section. No \
         occurrence, placement, or component reference is transferred.",
    );
}

#[test]
fn embedded_documents_and_mate_lists_are_reported_as_not_transferred() {
    let scan = synthetic_scan(
        vec![
            named_block(super::TREE_SECTION, tree().into_bytes()),
            named_block("ImportedComp/Pin.SLDPRT", vec![0]),
            named_block(
                "Contents/Config-0-MatesList",
                b"\0\0\0\0\0\0moMateCoincident".to_vec(),
            ),
        ],
        Vec::new(),
    );
    let projection = super::read(&ctx(), &scan)
        .expect("read")
        .expect("an assembly document");
    assert_eq!(projection.occurrences().len(), 6);
    let codes = codes(&projection);
    for code in [
        SldprtLossCode::AssemblyEmbeddedDocumentsNotTransferred,
        SldprtLossCode::AssemblyMatesNotTransferred,
    ] {
        assert!(codes.contains(&code.kind().local_code().to_string()));
    }
}

/// A component tree whose root model 2 places the given `swReference`
/// elements, with part model 8 and the extra models given.
fn tree_with(references: &str, models: &str) -> String {
    format!(
        r#"<swSolidWorks>
 <swHeader>
  <swFile id="3" swDocType="ASSEMBLY" swPath="C:\work\Top.SLDASM"/>
  <swFile id="6" swDocType="ASSEMBLY" swPath="C:\work\Sub.SLDASM"/>
  <swFile id="9" swDocType="PART" swPath="C:\work\Pin.SLDPRT"/>
 </swHeader>
 <swModelList>
  <swModel id="8" swFileRef="9"/>
  {models}
  <swModel id="2" swFileRef="3">{references}</swModel>
 </swModelList>
 <swConfigurationList>
  <swConfiguration id="1" swName="Default" swMostRecentConfiguration="YES" swModelRef="2"/>
 </swConfigurationList>
</swSolidWorks>"#
    )
}

/// One `swReference` to `model` with the identity `id`.
fn reference(id: &str, model: &str, transform: &str) -> String {
    format!(
        r#"<swReference id="{id}" swName="Pin" swReferenceNumber="{id}" swModelRef="{model}" swTransform="{transform}"/>"#
    )
}

#[test]
fn an_invalid_or_repeated_reference_is_omitted_and_reported() {
    let references = [
        reference("4", "8", IDENTITY),
        reference("4", "8", IDENTITY),
        reference("5", "8", "1 0 0"),
        reference("6", "8", "2 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1"),
        reference("a b", "8", IDENTITY),
        r#"<swReference swName="Pin" swModelRef="8"/>"#.to_owned(),
    ]
    .concat();
    let projection = project(&ctx(), &tree_with(&references, "")).expect("project");
    assert_eq!(projection.occurrences().len(), 2);
    let unresolved = projection
        .losses()
        .iter()
        .find(|loss| loss.code == SldprtLossCode::AssemblyPlacementUnresolved.kind())
        .expect("the unresolved loss");
    assert!(unresolved.message.starts_with("5 component instance(s)"));
}

#[test]
fn a_reference_cycle_is_omitted_and_reported() {
    let models = format!(
        r#"<swModel id="5" swFileRef="6">{}</swModel>"#,
        reference("7", "5", IDENTITY)
    );
    let projection =
        project(&ctx(), &tree_with(&reference("4", "5", IDENTITY), &models)).expect("project");
    assert_eq!(projection.occurrences().len(), 2);
    assert!(codes(&projection).contains(
        &SldprtLossCode::AssemblyPlacementUnresolved
            .kind()
            .local_code()
            .to_string()
    ));
}

#[test]
fn a_deep_sub_assembly_chain_is_refused_by_the_nesting_limit() {
    let depth = 10_000;
    let mut models = String::new();
    for level in 0..depth {
        let next = reference(&format!("r{level}"), &format!("m{}", level + 1), IDENTITY);
        std::fmt::Write::write_fmt(
            &mut models,
            format_args!(r#"<swModel id="m{level}" swFileRef="6">{next}</swModel>"#),
        )
        .expect("write to a string");
    }
    let text = tree_with(&reference("r", "m0", IDENTITY), &models);
    let error = project(&ctx(), &text).expect_err("the nesting limit refuses the chain");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn an_assembly_document_type_without_instances_is_a_local_definition() {
    let models = r#"<swModel id="5" swFileRef="6"/>"#;
    let projection =
        project(&ctx(), &tree_with(&reference("4", "5", IDENTITY), models)).expect("project");
    assert!(matches!(
        occurrence(&projection, "sldprt:assembly:occurrence#4").prototype,
        PrototypeReference::Local { .. }
    ));
}

#[test]
fn a_bill_of_materials_exclusion_and_visibility_are_reported() {
    let references = format!(
        r#"<swReference id="4" swName="Pin" swReferenceNumber="1" swModelRef="8" swHidden="NO" swExcludeFromBOM="YES" swTransform="{IDENTITY}"/>"#
    );
    let projection = project(&ctx(), &tree_with(&references, "")).expect("project");
    assert_eq!(
        occurrence(&projection, "sldprt:assembly:occurrence#4").visible,
        Some(true)
    );
    assert!(codes(&projection).contains(
        &SldprtLossCode::AssemblyBomExclusionNotTransferred
            .kind()
            .local_code()
            .to_string()
    ));
}

#[test]
fn a_tree_that_is_not_xml_states_the_exact_structure_loss() {
    let projection = project(&ctx(), "<swSolidWorks>").expect("project");
    assert_structure_loss(
        &projection,
        "The `swXmlContents/COMPINSTANCETREE` section is not well-formed XML. No occurrence is \
         transferred.",
    );
}

#[test]
fn a_container_only_decode_reads_no_assembly_structure() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let context = cadmpeg_core::decode::DecodeContext::new(
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
        cadmpeg_core::decode::DecodeDepth::Container,
    );
    let scan = synthetic_scan(
        vec![named_block(super::TREE_SECTION, tree().into_bytes())],
        Vec::new(),
    );
    assert!(super::read(&context, &scan).expect("read").is_none());
}

#[test]
fn a_mate_list_without_a_mate_class_makes_an_assembly_without_a_mates_loss() {
    let scan = synthetic_scan(
        vec![
            named_block(super::TREE_SECTION, tree().into_bytes()),
            named_block("contents\\config-0-mateslist", vec![0; 6]),
        ],
        Vec::new(),
    );
    let projection = super::read(&ctx(), &scan)
        .expect("read")
        .expect("an assembly document");
    assert!(!codes(&projection).contains(
        &SldprtLossCode::AssemblyMatesNotTransferred
            .kind()
            .local_code()
            .to_string()
    ));
}

#[test]
fn wrapped_section_names_are_the_tree_a_mate_list_and_an_embedded_document() {
    let scan = synthetic_scan(
        vec![
            named_block("swXmlContents/COMPINSTANCETREE__ZLB", tree().into_bytes()),
            named_block("Box.SLDASM__ZLB", vec![0]),
            named_block(
                "Contents/Config-0-MatesList__ZLB",
                b"\0\0\0\0\0\0moMateCoincident".to_vec(),
            ),
        ],
        Vec::new(),
    );
    let projection = super::read(&ctx(), &scan)
        .expect("read")
        .expect("an assembly document");
    assert_eq!(projection.occurrences().len(), 6);
    let codes = codes(&projection);
    for code in [
        SldprtLossCode::AssemblyEmbeddedDocumentsNotTransferred,
        SldprtLossCode::AssemblyMatesNotTransferred,
    ] {
        assert!(codes.contains(&code.kind().local_code().to_string()));
    }
}

#[test]
fn a_document_with_different_trees_states_the_exact_structure_loss() {
    let scan = synthetic_scan(
        vec![
            named_block(super::TREE_SECTION, tree().into_bytes()),
            named_block(
                "swXmlContents/COMPINSTANCETREE__ZLB",
                tree_with("", "").into_bytes(),
            ),
        ],
        Vec::new(),
    );
    let projection = super::read(&ctx(), &scan)
        .expect("read")
        .expect("an assembly document");
    assert_structure_loss(
        &projection,
        "The document holds different `swXmlContents/COMPINSTANCETREE` sections. No occurrence \
         is transferred.",
    );
}

#[test]
fn a_repeated_model_identity_states_the_exact_structure_loss() {
    let models = r#"<swModel id="8" swFileRef="9"/>"#;
    let projection =
        project(&ctx(), &tree_with(&reference("4", "8", IDENTITY), models)).expect("project");
    assert_structure_loss(
        &projection,
        "The component tree repeats the `swModel` id `8`. No occurrence is transferred.",
    );
}

#[test]
fn a_model_without_a_document_type_that_places_instances_is_a_sub_assembly() {
    let models = format!(
        r#"<swModel id="5">{}</swModel>"#,
        reference("7", "8", IDENTITY)
    );
    let projection =
        project(&ctx(), &tree_with(&reference("4", "5", IDENTITY), &models)).expect("project");
    assert!(matches!(
        occurrence(&projection, "sldprt:assembly:occurrence#4").prototype,
        PrototypeReference::Local { .. }
    ));
}

#[test]
fn a_mirrored_placement_is_omitted_and_reported_as_mirrored() {
    let mirrored = "-1 0 0 0 0 1 0 0 0 0 1 0 0 0 0 1";
    let projection =
        project(&ctx(), &tree_with(&reference("4", "8", mirrored), "")).expect("project");
    assert!(projection
        .occurrences()
        .iter()
        .all(|occurrence| occurrence.id.as_str() != "sldprt:assembly:occurrence#4"));
    assert!(projection.losses().iter().any(|loss| loss.code
        == SldprtLossCode::AssemblyPlacementUnresolved.kind()
        && loss
            .message
            .contains("1 component instance(s) have a mirrored placement")));
}
