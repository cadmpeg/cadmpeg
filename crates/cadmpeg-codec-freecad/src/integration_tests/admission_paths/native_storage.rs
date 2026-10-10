// SPDX-License-Identifier: Apache-2.0
//! Classification and admission of nonempty native populations.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, View};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_ir::codec::CodecBackend;
use crate::native::{ObjectRecord, PropertyRecord};

const DOCUMENT: &str = r#"<Document SchemaVersion="4" FileVersion="1">
<Objects Count="6"><Object type="App::Feature" name="A"/><Object type="App::Part" name="Part"/><Object type="App::FeaturePython" name="Ground"/><Object type="TechDraw::DrawPage" name="Page"/><Object type="App::Annotation" name="Note"/><Object type="Part::Feature" name="Attachment"/></Objects>
<ObjectData Count="6">
<Object name="A"><Properties Count="1"><Property name="P" type="App::PropertyString"><String value="native value"/></Property></Properties></Object>
<Object name="Part"><Properties Count="0"/></Object>
<Object name="Ground"><Properties Count="1"><Property name="ObjectToGround" type="App::PropertyLinkGlobal"><Link value="Part"/></Property></Properties></Object>
<Object name="Page"><Properties Count="0"/></Object>
<Object name="Note"><Properties Count="1"><Property name="Text" type="App::PropertyStringList"><StringList count="1"><String value="note"/></StringList></Property></Properties></Object>
<Object name="Attachment"><Properties Count="1"><Property name="MapMode" type="App::PropertyEnumeration"><Integer value="0"/></Property></Properties></Object>
</ObjectData></Document>"#;
const GUI: &str = r#"<Document SchemaVersion="1" extra="native attribute"><Camera settings=""/><ViewProviderData Count="1"><ViewProvider name="Part"><Properties Count="1"><Property name="Label" type="App::PropertyString"><String value="native label"/></Property></Properties></ViewProvider></ViewProviderData></Document>"#;

fn archive() -> Vec<u8> {
    crate::test_support::test_archive::archive_entries(&[("Document.xml", DOCUMENT.as_bytes()), ("GuiDocument.xml", GUI.as_bytes())])
}

fn assert_native_population_is_scoped(operation: &str, text_operation: &str) {
    let bytes = archive();
        // Prove the nonempty producer reaches this admission, independently of classification.
        crate::test_support::refusal_at(ResourceDimension::CollectionItems, &bytes, operation, |ctx| {
            crate::FcstdCodec.decode_impl(ctx, View::over_retained(&bytes)).map(|_| ())
        });
    crate::test_support::refusal_at(ResourceDimension::WorkUnits, &bytes, text_operation, |ctx| {
        crate::FcstdCodec.decode_impl(ctx, View::over_retained(&bytes)).map(|_| ())
    });
    for retained_operation in [operation, text_operation] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, retained_operation, None);
        let decoded = crate::FcstdCodec.decode_impl(&ctx, root).expect("native storage is scoped");
        drop(probe);
        assert_eq!(ctx.resource_refusal(), None);
        let namespace = decoded.ir.native.namespace("fcstd").expect("native namespace");
        for arena in ["objects", "properties", "entries", "product_nodes", "joints", "drawings", "annotations", "attachments", "gui_documents", "gui_view_providers", "gui_properties"] {
            assert!(!namespace.arenas()[arena].is_empty(), "nonempty {arena}");
        }
        let properties: Vec<PropertyRecord> = namespace.arena_as("properties").expect("typed properties");
        let value = properties.iter().find(|property| property.name == "P").expect("native property");
        assert_eq!(value.values()[0].attributes["value"], "native value");
        let documents: Vec<crate::native::GuiDocumentRecord> = namespace.arena_as("gui_documents").expect("typed GUI document");
        assert_eq!(documents[0].attributes["extra"], "native attribute");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx.reserve_scoped(u64::MAX, "returned native scratch").expect_err("finite materialized cap") else { panic!("materialized refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.used, 0);
        assert_eq!(value.values()[0].attributes["value"], "native value");
    }
}

#[test]
fn decode_nonempty_native_objects_remain_scoped() {
    assert_native_population_is_scoped("FCStd object records", "FCStd object name");
}

#[test]
fn decode_nonempty_native_properties_remain_scoped() {
    assert_native_population_is_scoped("FCStd persisted property records", "FCStd persisted property XML");
}

#[test]
fn decode_nonempty_native_entries_remain_scoped() {
    assert_native_population_is_scoped("FCStd entry records", "FCStd entry record name");
}

#[test]
fn decode_nonempty_native_products_remain_scoped() {
    assert_native_population_is_scoped("fcstd product records", "fcstd product object");
}

#[test]
fn decode_nonempty_native_joints_remain_scoped() {
    assert_native_population_is_scoped("fcstd joint records", "fcstd joint object");
}

#[test]
fn decode_nonempty_native_drawings_remain_scoped() {
    assert_native_population_is_scoped("fcstd drawing records", "fcstd drawing object");
}

#[test]
fn decode_nonempty_native_annotations_remain_scoped() {
    assert_native_population_is_scoped("fcstd annotation records", "fcstd annotation object");
}

#[test]
fn decode_nonempty_native_attachments_remain_scoped() {
    assert_native_population_is_scoped("FreeCAD attachment records", "FreeCAD attachment object");
}

#[test]
fn decode_nonempty_native_gui_documents_remain_scoped() {
    assert_native_population_is_scoped("FCStd GUI document records", "FCStd GUI document attribute");
}

#[test]
fn decode_nonempty_native_gui_providers_remain_scoped() {
    assert_native_population_is_scoped("FCStd GUI provider records", "FCStd GUI provider XML");
}

#[test]
fn decode_nonempty_native_gui_properties_remain_scoped() {
    assert_native_population_is_scoped("FCStd GUI property records", "FCStd GUI property XML");
}

#[derive(Clone, Copy)]
enum Producer { Product, Joint, Drawing, Annotation, Attachment }

impl Producer {
    fn run(self, ctx: &DecodeContext<'_>, objects: &[ObjectRecord], properties: &[PropertyRecord]) -> Result<(), cadmpeg_core::CodecError> {
        match self {
            Self::Product => ctx.with_scoped_storage("native product test", || crate::product::transfer(ctx, objects, properties, &std::collections::BTreeMap::new()).map(|records| { assert!(!records.is_empty()); })).map(|_| ()),
            Self::Joint => ctx.with_scoped_storage("native joint test", || crate::joint::transfer(ctx, objects, properties).map(|records| { assert!(!records.is_empty()); })).map(|_| ()),
            Self::Drawing => ctx.with_scoped_storage("native drawing test", || crate::drawing::transfer(ctx, objects, properties).map(|records| { assert!(!records.is_empty()); })).map(|_| ()),
            Self::Annotation => ctx.with_scoped_storage("native annotation test", || crate::annotation::transfer(ctx, objects, properties).map(|records| { assert!(!records.is_empty()); })).map(|_| ()),
            Self::Attachment => ctx.with_scoped_storage("native attachment test", || crate::attachment::transfer(ctx, objects, properties).map(|records| { assert!(!records.is_empty()); })).map(|_| ()),
        }
    }
}

fn assert_native_producer_admission(producer: Producer, work: &str, storage: &str) {
    let bytes = archive();
    let (mut objects, mut properties) = crate::test_support::with_service_context(&bytes, |ctx| {
        let decoded = crate::FcstdCodec.decode_impl(ctx, View::over_retained(&bytes)).expect("fixture decode");
        let namespace = decoded.ir.native.namespace("fcstd").expect("namespace");
        (namespace.arena_as::<ObjectRecord>("objects").expect("objects"), namespace.arena_as::<PropertyRecord>("properties").expect("properties"))
    });
    let templates = objects.clone();
    let property_templates = properties.clone();
    for ordinal in 0..64 {
        for template in &templates {
            let name = format!("{}_{ordinal}", template.name());
            let owner = format!("fcstd:native:object#{name}");
            let mut object = template.clone();
            object.identity = crate::native::object_identity::ObjectIdentity::try_new(owner.clone(), name.clone()).expect("fixture object identity");
            object.order = objects.len();
            objects.push(object);
            for property in property_templates.iter().filter(|property| property.owner.as_str() == template.id().as_str()) {
                let mut property = property.clone();
                property.id = format!("fcstd:native:property#{name}:{}", property.name);
                property.owner = owner.clone();
                properties.push(property);
            }
        }
    }
        crate::test_support::refusal_at(ResourceDimension::WorkUnits, &[], work, |ctx| producer.run(ctx, &objects, &properties));
        crate::test_support::materialized_refusal_at(storage, |ctx| producer.run(ctx, &objects, &properties));
}

#[test]
fn native_product_producer_charges_work_and_materialized_storage() {
    assert_native_producer_admission(Producer::Product, "fcstd product object", "fcstd product records");
}

#[test]
fn native_joint_producer_charges_work_and_materialized_storage() {
    assert_native_producer_admission(Producer::Joint, "fcstd joint object", "fcstd joint records");
}

#[test]
fn native_drawing_producer_charges_work_and_materialized_storage() {
    assert_native_producer_admission(Producer::Drawing, "fcstd drawing object", "fcstd drawing records");
}

#[test]
fn native_annotation_producer_charges_work_and_materialized_storage() {
    assert_native_producer_admission(Producer::Annotation, "fcstd annotation object", "fcstd annotation records");
}

#[test]
fn native_attachment_producer_charges_work_and_materialized_storage() {
    assert_native_producer_admission(Producer::Attachment, "FreeCAD attachment object", "FreeCAD attachment records");
}

