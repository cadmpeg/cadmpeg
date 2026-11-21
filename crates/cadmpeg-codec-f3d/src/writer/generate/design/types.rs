// SPDX-License-Identifier: Apache-2.0
//! The type-table rows of the records a design scope's form writes, with the
//! GUID spellings a magic-1234 Design `MetaStream` stores (f3d.md §3.1,
//! "Records of a sketch-and-extrude design" and "Records of a Base Feature
//! design"). The rows with no record of their own are the ancestors that the
//! paired headers name and the base types of the written types: a Base Feature
//! design's type table lists them with an empty entity-ID run.

use crate::writer::generate::presentation::TypeRow;

const DATA_MODEL_ROOT_GUID: &str = "98542EB9-A4F2-4137-A808-DBB5B3CD6159";
const DATA_MODEL_OBJECT_GUID: &str = "CB844AB6-240D-4fc9-9C9F-3679DC896D6F";
const COLLECTION_BASE_GUID: &str = "834C9DEF-4C39-4587-A08D-A5BD1267B7B4";
const COMMON_DATA_COLLECTION_GUID: &str = "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081";
const PROPERTY_OWNER_GUID: &str = "63D6AD42-466A-40B7-9CCD-EB21BC500EF6";
const ORIGIN_ENTITY_GUID: &str = "1E5F6CE7-7544-4DE1-8C7B-6BACF6DC8E40";

const fn row(
    guid: &'static str,
    base: &'static str,
    version: u32,
    module: &'static str,
) -> TypeRow {
    TypeRow {
        guid,
        base: Some(base),
        version,
        module,
    }
}

/// A user parameter.
pub(super) const PARAMETER: TypeRow = row(
    "7A6A3D31-BE74-4E19-9115-20642944C2E4",
    PROPERTY_OWNER_GUID,
    8,
    "CommonData",
);
pub(super) const TIMELINE: TypeRow = row(
    "2F4C1849-1A5A-4F6C-A086-8DD445CBF94B",
    DATA_MODEL_ROOT_GUID,
    3,
    "Fusion",
);
pub(super) const TIMELINE_CONTEXT: TypeRow = row(
    "C3423E04-7E85-4B44-8E31-A5FA9C100052",
    DATA_MODEL_OBJECT_GUID,
    2,
    "Fusion",
);
pub(super) const BASE_FEATURE: TypeRow = row(
    "D7015B81-AB4D-4C89-BED7-AA4411E1964A",
    "2FCB0587-233E-449B-9724-9AAE5AA23647",
    4,
    "Fusion",
);
pub(super) const RESULT_OWNER: TypeRow = row(
    "493A89BA-E146-498D-9033-2783FE727AF4",
    "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
    3,
    "Fusion",
);
pub(super) const BODY_TRACKING: TypeRow = row(
    "5988EB6C-011B-4012-B500-089A713F57BF",
    "5C069ACB-169F-4555-9792-815B4671DDA0",
    2,
    "Fusion",
);
pub(super) const TRACKED_RESULTS: TypeRow = row(
    "B7F34D7B-DAC4-41A5-B67D-D672537C3BFA",
    DATA_MODEL_OBJECT_GUID,
    2,
    "Fusion",
);
pub(super) const ITEM_TAIL_COMPANION: TypeRow = row(
    "5FBE538E-139B-46D4-BCAF-CC796D1E1F78",
    DATA_MODEL_ROOT_GUID,
    0,
    "Fusion",
);
pub(super) const RECORD_TIMESTAMPS: TypeRow = row(
    "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
    DATA_MODEL_ROOT_GUID,
    1,
    "Fusion",
);
pub(super) const OWNER_COLLECTION: TypeRow = row(
    "3A6D1E62-99F8-4E32-BAAA-DBA0A2CFB27C",
    COLLECTION_BASE_GUID,
    0,
    "Fusion",
);
pub(super) const COMPONENT_RECORD: TypeRow = row(
    "5D15E992-1E93-4D79-A553-10D2624148AE",
    "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
    1,
    "Fusion",
);
pub(super) const CONTEXT_COMPONENT_LINK: TypeRow = row(
    "549FBB80-B890-473E-A5C0-415D3D9BF4E6",
    DATA_MODEL_ROOT_GUID,
    4,
    "Fusion",
);
pub(super) const CONTEXT_12D00AC0: TypeRow = row(
    "12D00AC0-B828-48C8-9740-80C4D992DB65",
    DATA_MODEL_ROOT_GUID,
    0,
    "Fusion",
);
pub(super) const CONTEXT_016D8C93: TypeRow = row(
    "016D8C93-144B-4352-9299-98FA738BD4FF",
    DATA_MODEL_ROOT_GUID,
    2,
    "Fusion",
);
pub(super) const CONTEXT_MEMBERS: TypeRow = row(
    "B65B5682-9355-44C7-8AFC-823058C4A36B",
    DATA_MODEL_ROOT_GUID,
    0,
    "Fusion",
);
pub(super) const DEPENDENCIES: TypeRow = row(
    "B13113CC-031E-4F93-8240-C4C10728869F",
    DATA_MODEL_ROOT_GUID,
    0,
    "Fusion",
);
pub(super) const PERSISTENT_IDS: TypeRow = row(
    "5D89B935-3F04-4CAB-81F5-E7B309E7E13F",
    DATA_MODEL_ROOT_GUID,
    1,
    "Fusion",
);
pub(super) const ORIGIN: TypeRow = row(
    "2972A3B8-3627-49BC-8C3D-18707605EC44",
    COMMON_DATA_COLLECTION_GUID,
    1,
    "Geometry",
);
pub(super) const ORIGIN_POINT: TypeRow = row(
    "0E18EEF2-0D6F-480C-997E-5EBC7369AE5F",
    ORIGIN_ENTITY_GUID,
    0,
    "Geometry",
);
pub(super) const ORIGIN_AXIS: TypeRow = row(
    "19031004-3AB6-4D13-ABBF-C51EE214688D",
    ORIGIN_ENTITY_GUID,
    0,
    "Geometry",
);
pub(super) const ORIGIN_PLANE: TypeRow = row(
    "8D424E29-DE23-4B1F-A3AD-84C52A497D1A",
    ORIGIN_ENTITY_GUID,
    1,
    "Geometry",
);
pub(super) const PARAMETER_COLLECTION: TypeRow = row(
    "B0556F58-EE09-46A7-B18F-E296F48E5B13",
    COMMON_DATA_COLLECTION_GUID,
    4,
    "CommonData",
);
pub(super) const COLLECTION_COMPANION: TypeRow = row(
    "E276745D-D84E-40D5-88BC-8C94DD4ACCDF",
    DATA_MODEL_ROOT_GUID,
    1,
    "CommonData",
);
pub(super) const COMPONENT_AGGREGATE: TypeRow = row(
    "E03784ED-5E19-4E14-B9F2-3B07017018CD",
    "42054630-20A0-40E1-B969-CFE9E742F5C9",
    23,
    "Fusion",
);
pub(super) const BODY_REFERENCE: TypeRow = row(
    "D3937028-C20C-4E65-B010-94AD418A5C20",
    PROPERTY_OWNER_GUID,
    19,
    "Body",
);

/// The ancestor that the paired header of most records names.
pub(super) const DATA_MODEL_OBJECT: TypeRow =
    row(DATA_MODEL_OBJECT_GUID, DATA_MODEL_ROOT_GUID, 1, "DataModel");
/// The ancestor that a body tracking entry's paired header names.
pub(super) const TRACKING_ENTRY: TypeRow = row(
    "5C069ACB-169F-4555-9792-815B4671DDA0",
    DATA_MODEL_OBJECT_GUID,
    1,
    "EntityTracking",
);
/// The ancestor that the owner collection's paired header names.
pub(super) const COLLECTION: TypeRow =
    row(COLLECTION_BASE_GUID, DATA_MODEL_OBJECT_GUID, 2, "DataModel");
/// The ancestor that the component aggregate's paired header names.
pub(super) const AGGREGATE: TypeRow = row(
    "42054630-20A0-40E1-B969-CFE9E742F5C9",
    "EAF14CD0-75FD-4770-9C01-ABF1A66C2BCA",
    16,
    "Component",
);

/// The other ancestors a Base Feature design's type table lists.
pub(super) const ANCESTORS: [TypeRow; 7] = [
    TypeRow {
        guid: DATA_MODEL_ROOT_GUID,
        base: None,
        version: 0,
        module: "",
    },
    row(
        "2FCB0587-233E-449B-9724-9AAE5AA23647",
        "2EDC2536-D95B-4469-8ACA-DC07124D9C43",
        0,
        "Fusion",
    ),
    row(
        "D0D7E795-ECF7-48A8-A7A3-F983C5A29F8B",
        "A0A15D26-1F3B-4120-A3F1-9CDDA189AB74",
        0,
        "Fusion",
    ),
    row(
        ORIGIN_ENTITY_GUID,
        "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
        5,
        "Geometry",
    ),
    row(
        COMMON_DATA_COLLECTION_GUID,
        COLLECTION_BASE_GUID,
        0,
        "CommonData",
    ),
    row(PROPERTY_OWNER_GUID, DATA_MODEL_OBJECT_GUID, 0, "CommonData"),
    row(
        "E8DCB040-6A0E-4AD1-BCF3-1E1AEDD35EE4",
        "130A0711-4E92-4FCD-AADE-B9C82238BB27",
        1,
        "Body",
    ),
];

/// The `Sketch` scope.
pub(super) const SKETCH_SCOPE: TypeRow = row(
    "8DA771B7-52ED-42FC-94E2-2353FE141373",
    "A2052F46-21B4-4AAA-9AF1-C371F0AB6CAB",
    5,
    "Fusion",
);
/// The sketch container; its Geometry member has the container's index.
pub(super) const SKETCH_CONTAINER: TypeRow = row(
    "44A64366-4BD3-4B24-881A-F94C206E8F2D",
    SKETCH_GEOMETRY_MEMBER_GUID,
    18,
    "MSketch",
);
/// The sketch frame record.
pub(super) const SKETCH_FRAME: TypeRow = row(
    "F47A46FB-DA27-4AFA-8A72-1B38FA596E23",
    PROPERTY_OWNER_GUID,
    0,
    "Geometry",
);
pub(super) const SKETCH_POINT: TypeRow = row(
    "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB",
    "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
    11,
    "Geometry",
);
pub(super) const SKETCH_POINT_COMPANION: TypeRow = row(
    "362B7EC3-0F09-47C8-A3BE-DC066715CDAE",
    PROPERTY_OWNER_GUID,
    0,
    "Geometry",
);
pub(super) const SKETCH_LINE: TypeRow = row(
    "DCA267ED-D615-4934-B64F-AD805E8003E2",
    SKETCH_CURVE_GUID,
    2,
    "Geometry",
);
pub(super) const SKETCH_CIRCULAR_CURVE: TypeRow = row(
    "F0130424-8B7E-4092-93C9-1CA807482534",
    SKETCH_CURVE_GUID,
    0,
    "Geometry",
);
pub(super) const SKETCH_RELATION: TypeRow =
    row(SKETCH_RELATION_GUID, DATA_MODEL_ROOT_GUID, 3, "MSketch");
pub(super) const SKETCH_TANGENCY: TypeRow = row(
    "24DB790E-3DCD-4336-AFA3-6F119EF2239B",
    SKETCH_RELATION_GUID,
    0,
    "MSketch",
);
pub(super) const SKETCH_PLACEMENT: TypeRow = row(
    "7B6C3C2D-4096-4E6D-B225-699E8C2D9355",
    "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
    14,
    "Fusion",
);
pub(super) const PLACEMENT_PATH: TypeRow = row(
    "5A1BF548-241F-46FD-9FB5-E4B05126EB9D",
    "6144EA59-3ACA-4172-9F14-A6531E94D69B",
    1,
    "Fusion",
);
pub(super) const TRACKED_IDENTITY: TypeRow = row(
    "90055C05-546C-4EE7-B3C9-3DD922AD0C9C",
    "70F5E801-1E8E-436E-8A09-36487F8A2859",
    0,
    "EntityTracking",
);
pub(super) const PLACEMENT_TRANSFORM: TypeRow = row(
    "CC54ECAD-6DE8-4DE1-803D-BBE0E751975B",
    "9A171A9A-5599-4872-987C-9B5E8E2A6C9C",
    0,
    "EntityTracking",
);
pub(super) const TRANSFORM_TARGET: TypeRow = row(
    "CB4F4CC2-9D6F-48BA-9B1F-20C1992C9C9C",
    "5C069ACB-169F-4555-9792-815B4671DDA0",
    2,
    "EntityTracking",
);
pub(super) const PLACEMENT_TRACKING_FLAG: TypeRow = row(
    "2DF7DA30-A4E4-4260-AFD1-D9C7154CB44B",
    "872045B4-F653-4691-9752-ABC7684834BF",
    0,
    "EntityTracking",
);
pub(super) const SKETCHES_ROOT: TypeRow = row(
    "3A7E4411-2E8A-4FA8-96A3-9902E1ACAF2F",
    "5EAF2F0F-4AEE-4f20-B569-119A7BD1F36D",
    0,
    "Geometry",
);
/// The type of a sketch container's Geometry member.
pub(super) const SKETCH_GEOMETRY_MEMBER: TypeRow = row(
    SKETCH_GEOMETRY_MEMBER_GUID,
    COMMON_DATA_COLLECTION_GUID,
    4,
    "Geometry",
);

const SKETCH_GEOMETRY_MEMBER_GUID: &str = "37AD519C-AFB3-4CE2-9E6D-E3269FC6CDB9";
const SKETCH_CURVE_GUID: &str = "9FF71F46-205F-484B-A2B5-A245D0D8FE33";
const SKETCH_RELATION_GUID: &str = "60403D47-0C49-49B0-BDE8-1679608164A2";

/// The ancestors of the records of a `Sketch` item that a design with
/// `Sketch` items lists with an empty entity-ID run (f3d.md §3.1, "Records
/// of a Sketch item").
pub(super) const SKETCH_ANCESTORS: [TypeRow; 9] = [
    SKETCH_GEOMETRY_MEMBER,
    row(
        SKETCH_CURVE_GUID,
        "8E9D588C-28EF-43C4-B9A6-AA5430A68D4B",
        9,
        "Geometry",
    ),
    row(
        "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
        "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F",
        1,
        "Fusion",
    ),
    row(
        "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F",
        "E8BDB9D4-2E39-405C-B05E-E2206AD3F125",
        2,
        "Fusion",
    ),
    row(
        "6144EA59-3ACA-4172-9F14-A6531E94D69B",
        "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F",
        0,
        "Fusion",
    ),
    row(
        "70F5E801-1E8E-436E-8A09-36487F8A2859",
        DATA_MODEL_ROOT_GUID,
        0,
        "EntityTracking",
    ),
    row(
        "9A171A9A-5599-4872-987C-9B5E8E2A6C9C",
        "872045B4-F653-4691-9752-ABC7684834BF",
        0,
        "EntityTracking",
    ),
    row(
        "872045B4-F653-4691-9752-ABC7684834BF",
        "FA85BB02-9D57-4E5E-A3A1-3D150C7ED082",
        1,
        "EntityTracking",
    ),
    row(
        "FA85BB02-9D57-4E5E-A3A1-3D150C7ED082",
        DATA_MODEL_ROOT_GUID,
        1,
        "EntityTracking",
    ),
];
