// SPDX-License-Identifier: Apache-2.0

use super::super::{
    linked_planar_profiles, preceding_profile_feature_dependencies, profile_ref,
    ProfileFeatureOrder,
};
use crate::native::{
    LinkTarget, LinkTargetWire, ObjectRecord, PropertyBody, PropertyFamily, PropertyRecord,
    RetainedXml,
};
use cadmpeg_ir::features::{FeatureId, PlanarProfileRef, ProfileRef};
use cadmpeg_ir::{Codec, DecodeOptions};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Cursor;

fn linked_profile(owner: &str, target: &str) -> PropertyRecord {
    let link = LinkTarget::optional_from_wire(LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some(target.to_owned()),
        subelements: Vec::new(),
    })
    .expect("valid profile link");
    PropertyRecord {
        id: "native-profile-property".to_owned(),
        owner: owner.to_owned(),
        name: "Base".to_owned(),
        type_name: "App::PropertyLink".to_owned(),
        family: PropertyFamily::Link,
        status: None,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text("<Property/>".to_owned(), 0).expect("property XML"),
    }
}

fn section_list(owner: &str, id: &str, targets: Vec<LinkTarget>) -> PropertyRecord {
    PropertyRecord {
        id: id.to_owned(),
        owner: owner.to_owned(),
        name: "Sections".to_owned(),
        type_name: "App::PropertyXLinkList".to_owned(),
        family: PropertyFamily::Link,
        status: None,
        body: PropertyBody::Persisted {
            values: Vec::new(),
            links: targets.into_iter().map(Some).collect(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: RetainedXml::from_text("<Property/>".to_owned(), 0).expect("property XML"),
    }
}

fn profile_link(document: Option<&str>, object: &str) -> LinkTarget {
    LinkTarget::optional_from_wire(LinkTargetWire {
        document: document.map(str::to_owned),
        document_attribute: document.map(|_| "file".to_owned()),
        object: Some(object.to_owned()),
        subelements: Vec::new(),
    })
    .expect("valid profile link")
    .expect("profile link target")
}

fn stays_native(owner: &str, target: &str, dependencies: &HashMap<(&str, &str), FeatureId>) {
    let property = linked_profile(owner, target);
    assert_eq!(
        profile_ref(owner, &[&property], &HashMap::new(), dependencies),
        ProfileRef::Planar(PlanarProfileRef::Native(property.id))
    );
}

#[test]
fn profile_features_require_emitted_wire_outputs_and_preceding_dependencies() {
    let bytes =
        include_bytes!("../../../../../corpus/freecad_fcstd/fixtures/core_operations.FCStd");
    let decoded = crate::FcstdCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("authored operation fixture");
    let ir = decoded.ir();
    let native = ir.native.namespace("fcstd").unwrap();
    let objects = native.arena_as::<ObjectRecord>("objects").unwrap();
    let properties = native.arena_as::<PropertyRecord>("properties").unwrap();
    let payloads = native
        .arena_as::<crate::brep::ShapePayloadRecord>("shape_payloads")
        .unwrap();
    let mut properties_by_owner = HashMap::new();
    for property in &properties {
        properties_by_owner
            .entry(property.owner.as_str())
            .or_insert_with(Vec::new)
            .push(property);
    }
    let feature_ids = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native| (native, feature.id.clone()))
        })
        .collect::<HashMap<_, _>>();
    let ordinals = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native| (native, feature.ordinal))
        })
        .collect::<HashMap<_, _>>();
    let empty_cycles = BTreeSet::new();
    let empty_attachments = HashSet::new();
    let dependencies = preceding_profile_feature_dependencies(
        &objects,
        &properties_by_owner,
        &payloads,
        &ir.model.bodies,
        ProfileFeatureOrder {
            feature_ids: &feature_ids,
            ordinals: &ordinals,
        },
        &empty_cycles,
        &empty_attachments,
    );

    let extrusion = crate::native::native_id("object", "SymmetricExtrusion");
    let profile = crate::native::native_id("object", "ExtrusionProfile");
    assert_eq!(
        dependencies.get(&(extrusion.as_str(), profile.as_str())),
        feature_ids.get(profile.as_str())
    );
    let profile_property = properties_by_owner
        .get(extrusion.as_str())
        .unwrap()
        .iter()
        .find(|property| property.name == "Base")
        .unwrap();
    assert!(matches!(
        profile_ref(
            extrusion.as_str(),
            &[profile_property],
            &HashMap::new(),
            &dependencies
        ),
        ProfileRef::Planar(PlanarProfileRef::Feature(_))
    ));

    let owner_ordinal = ordinals[extrusion.as_str()];
    let mut forward_ordinals = ordinals.clone();
    forward_ordinals.insert(profile.as_str(), owner_ordinal + 1);
    let forward = preceding_profile_feature_dependencies(
        &objects,
        &properties_by_owner,
        &payloads,
        &ir.model.bodies,
        ProfileFeatureOrder {
            feature_ids: &feature_ids,
            ordinals: &forward_ordinals,
        },
        &empty_cycles,
        &empty_attachments,
    );
    stays_native(extrusion.as_str(), profile.as_str(), &forward);

    for cyclic in [extrusion.clone(), profile.clone()] {
        let cycles = BTreeSet::from([cyclic]);
        let cyclic_dependencies = preceding_profile_feature_dependencies(
            &objects,
            &properties_by_owner,
            &payloads,
            &ir.model.bodies,
            ProfileFeatureOrder {
                feature_ids: &feature_ids,
                ordinals: &ordinals,
            },
            &cycles,
            &empty_attachments,
        );
        stays_native(extrusion.as_str(), profile.as_str(), &cyclic_dependencies);
    }

    let mirror_base = crate::native::native_id("object", "MirrorBase");
    let mut with_noncurve_dependency = objects.clone();
    with_noncurve_dependency
        .iter_mut()
        .find(|object| object.id() == &extrusion)
        .unwrap()
        .dependencies
        .push(mirror_base.clone());
    let mut noncurve_ordinals = ordinals.clone();
    noncurve_ordinals.insert(extrusion.as_str(), 1);
    noncurve_ordinals.insert(mirror_base.as_str(), 0);
    let noncurve = preceding_profile_feature_dependencies(
        &with_noncurve_dependency,
        &properties_by_owner,
        &payloads,
        &ir.model.bodies,
        ProfileFeatureOrder {
            feature_ids: &feature_ids,
            ordinals: &noncurve_ordinals,
        },
        &empty_cycles,
        &empty_attachments,
    );
    assert!(!noncurve.contains_key(&(extrusion.as_str(), mirror_base.as_str())));
    stays_native(extrusion.as_str(), mirror_base.as_str(), &noncurve);

    let mut omitted_features = feature_ids.clone();
    omitted_features.remove(profile.as_str());
    let omitted = preceding_profile_feature_dependencies(
        &objects,
        &properties_by_owner,
        &payloads,
        &ir.model.bodies,
        ProfileFeatureOrder {
            feature_ids: &omitted_features,
            ordinals: &ordinals,
        },
        &empty_cycles,
        &empty_attachments,
    );
    stays_native(extrusion.as_str(), profile.as_str(), &omitted);
}

#[test]
fn external_section_links_do_not_alias_local_sketches_or_features() {
    let sketch = cadmpeg_ir::sketches::SketchId::mint("fcstd:model:sketch#Section")
        .expect("valid sketch identity");
    let feature =
        FeatureId::mint("fcstd:design:feature#SectionResult").expect("valid feature identity");
    let sketches = HashMap::from([("Section", sketch.clone())]);
    let feature_dependencies = HashMap::from([(("owner", "Section"), feature.clone())]);

    let sketch_property = section_list(
        "owner",
        "fcstd:native:property#sketch-sections",
        vec![
            profile_link(Some("external.FCStd"), "Section"),
            profile_link(None, "Section"),
        ],
    );
    assert_eq!(
        linked_planar_profiles(
            "owner",
            &[&sketch_property],
            &sketches,
            &feature_dependencies
        ),
        [
            PlanarProfileRef::Native(sketch_property.id.clone()),
            PlanarProfileRef::Sketch(sketch),
        ]
    );

    let feature_property = section_list(
        "owner",
        "fcstd:native:property#feature-sections",
        vec![
            profile_link(Some("external.FCStd"), "Section"),
            profile_link(None, "Section"),
        ],
    );
    assert_eq!(
        linked_planar_profiles(
            "owner",
            &[&feature_property],
            &HashMap::new(),
            &feature_dependencies
        ),
        [
            PlanarProfileRef::Native(feature_property.id.clone()),
            PlanarProfileRef::Feature(feature),
        ]
    );
}
