// SPDX-License-Identifier: Apache-2.0
//! Tools, presets, and machines (§7.6).
//!
//! The document tool table is canonical. Each field of [`NcNumbers`]
//! resolves to the preset's value, else that of the pocket holding the tool
//! in the setup's machine, else the tool's.

use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::setup::JobKind;
use super::technology::Technology;
use super::ProcessDimension;
use crate::design::construction::FrameAxis;
use crate::design::ids::{MachineId, PresetId, ToolId};
use crate::design::names::NativeName;
use crate::design::options::Hand;
use crate::design::units::UnitSymbol;
use crate::scalar::{Angle, Length};

/// An optional length that a process parameter can drive.
pub type PLength = Option<ProcessDimension<Length>>;

/// An optional angle that a process parameter can drive.
pub type PAngle = Option<ProcessDimension<Angle>>;

/// A cutting tool of the document tool table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// The tool's identity.
    pub id: ToolId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The ISO 14649-111 or 14649-121 tool class.
    pub class: ToolClass,
    /// The source's tool type, verbatim.
    pub native_kind: NativeName,
    /// The tool's identity in an external library.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_library"
    )]
    pub library: Option<LibraryRef>,
    /// The length unit the tool is authored in. Its dimensions are
    /// millimetres.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_unit"
    )]
    pub unit: Option<UnitSymbol>,
    /// The tool's NC numbers.
    pub nc: NcNumbers,
    /// The cutter's dimensions.
    pub cutter: Cutter,
    /// A turning tool's insert.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_insert"
    )]
    pub insert: Option<TurningInsert>,
    /// The holder's profile.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_holder"
    )]
    pub holder: Option<HolderProfile>,
    /// The tool's material.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_material"
    )]
    pub material: Option<NonBlankString>,
    /// The tool's feed per tooth, the default that a preset's per-tooth
    /// cutting feed overrides.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_chip_load"
    )]
    pub chip_load: PLength,
    /// The tool's vendor and the vendor's product id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_catalog"
    )]
    pub catalog: Option<ToolCatalog>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// The ISO 14649-111 and 14649-121 tool classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolClass {
    /// A flat end mill.
    FlatEndmill,
    /// A ball end mill.
    BallEndmill,
    /// A bull-nose end mill.
    BullnoseEndmill,
    /// A chamfer mill.
    ChamferMill,
    /// A face mill.
    FaceMill,
    /// A slot mill.
    SlotMill,
    /// A dovetail mill.
    DovetailMill,
    /// A thread mill.
    ThreadMill,
    /// A drill.
    Drill,
    /// A center drill.
    CenterDrill,
    /// A spot drill.
    SpotDrill,
    /// A countersink.
    Countersink,
    /// A counterbore.
    Counterbore,
    /// A reamer.
    Reamer,
    /// A tap.
    Tap,
    /// A boring bar.
    BoringBar,
    /// A general turning tool.
    TurningGeneral,
    /// A turning boring tool.
    TurningBoring,
    /// A turning grooving tool.
    TurningGrooving,
    /// A turning threading tool.
    TurningThreading,
    /// A probe.
    Probe,
    /// A jet cutter.
    JetCutter,
    /// A class that no arm states; the tool's `native_kind` holds the
    /// source's name.
    Unclassified,
}

/// A tool's identity in an external library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum LibraryRef {
    /// A GUID.
    Guid(Uuid),
    /// A path or asset id.
    Uri(NonBlankString),
}

/// A UUID of 16 bytes. It writes as its 36-character hyphenated lower-case
/// form, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (RFC 9562), and reading
/// accepts only that form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid(pub [u8; 16]);

impl Uuid {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];

    fn parse(text: &str) -> Option<Self> {
        let groups = text.split('-').collect::<Vec<_>>();
        if groups.len() != Self::GROUPS.len()
            || groups
                .iter()
                .zip(Self::GROUPS)
                .any(|(group, length)| group.len() != length)
        {
            return None;
        }
        let digits = groups.concat();
        let mut bytes = [0_u8; 16];
        for (byte, pair) in bytes.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
            let [high, low] = pair else {
                return None;
            };
            *byte = (nibble(*high)? << 4) | nibble(*low)?;
        }
        Some(Self(bytes))
    }
}

fn nibble(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                formatter.write_str("-")?;
            }
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for Uuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Uuid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            D::Error::custom("a Uuid is xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx in lower case")
        })
    }
}

#[cfg(feature = "schema")]
impl JsonSchema for Uuid {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Uuid".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "pattern": "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        })
    }
}

/// A tool's vendor and the vendor's product id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToolCatalog {
    /// The vendor.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vendor"
    )]
    pub vendor: Option<NonBlankString>,
    /// The vendor's product id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_product_id"
    )]
    pub product_id: Option<NonBlankString>,
    /// A free-text locator of the tool's catalog page, with no machining
    /// meaning.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_link"
    )]
    pub link: Option<NonBlankString>,
}

/// The NC numbers of a tool, a preset, or a machine pocket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct NcNumbers {
    /// The tool number.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_number"
    )]
    pub number: Option<ProcessDimension<u32>>,
    /// The length offset register.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_length_offset"
    )]
    pub length_offset: Option<ProcessDimension<u32>>,
    /// The diameter offset register.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_diameter_offset"
    )]
    pub diameter_offset: Option<ProcessDimension<u32>>,
    /// The turret.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_turret"
    )]
    pub turret: Option<ProcessDimension<u32>>,
}

/// The cutter's dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Cutter {
    /// The cutter diameter; a jet tool's kerf.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_diameter"
    )]
    pub diameter: PLength,
    /// The corner radius.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corner_radius"
    )]
    pub corner_radius: PLength,
    /// The flute length.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flute_length"
    )]
    pub flute_length: PLength,
    /// The overall length.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_overall_length"
    )]
    pub overall_length: PLength,
    /// The shank diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_shank_diameter"
    )]
    pub shank_diameter: PLength,
    /// The tip diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tip_diameter"
    )]
    pub tip_diameter: PLength,
    /// The thread pitch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_pitch"
    )]
    pub thread_pitch: PLength,
    /// The tip angle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tip_angle"
    )]
    pub tip_angle: PAngle,
    /// The taper angle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_taper_angle"
    )]
    pub taper_angle: PAngle,
    /// The number of flutes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_flutes"
    )]
    pub flutes: Option<ProcessDimension<u32>>,
    /// The cutter's hand.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_hand"
    )]
    pub hand: Option<Hand>,
}

/// A turning tool's insert, typed on its ISO 1832 designation. A dimension
/// the insert states is not repeated in the tool's cutter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TurningInsert {
    /// The shape letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_shape"
    )]
    pub shape: Option<char>,
    /// The relief letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_relief"
    )]
    pub relief: Option<char>,
    /// The tolerance letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tolerance"
    )]
    pub tolerance: Option<char>,
    /// The cross-section letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cross_section"
    )]
    pub cross_section: Option<char>,
    /// The inscribed-circle diameter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_inscribed_diameter"
    )]
    pub inscribed_diameter: PLength,
    /// The thickness.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thickness"
    )]
    pub thickness: PLength,
    /// The corner radius.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_corner_radius"
    )]
    pub corner_radius: PLength,
    /// The width.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_width"
    )]
    pub width: PLength,
    /// The tip angle.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tip_angle"
    )]
    pub tip_angle: PAngle,
    /// The thread pitch.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_thread_pitch"
    )]
    pub thread_pitch: PLength,
    /// The insert's relief angle, beside or in place of the relief letter.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_relief_angle"
    )]
    pub relief_angle: PAngle,
}

/// The profile of a tool holder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HolderProfile {
    /// The holder's sections.
    pub sections: Vec<HolderSection>,
}

/// One section of a tool holder's profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HolderSection {
    /// The diameter at the section's upper end.
    pub upper_diameter: Length,
    /// The diameter at the section's lower end.
    pub lower_diameter: Length,
    /// The section's length.
    pub length: Length,
}

/// A tool's usage with its own technology.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// The preset's identity.
    pub id: PresetId,
    /// The tool the preset uses.
    pub tool: ToolId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The preset's NC numbers.
    pub nc: NcNumbers,
    /// The preset's technology.
    pub technology: Technology,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// A machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// The machine's identity.
    pub id: MachineId,
    /// The source's name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_name"
    )]
    pub name: Option<String>,
    /// The machine's process family.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_job"
    )]
    pub job: Option<JobKind>,
    /// The machine's vendor.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_vendor"
    )]
    pub vendor: Option<NonBlankString>,
    /// The machine's model.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_model"
    )]
    pub model: Option<NonBlankString>,
    /// The machine's controller.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_control"
    )]
    pub control: Option<NonBlankString>,
    /// The linear axes the machine controls, in the machine's frame, where
    /// the source states them.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_linear_axes"
    )]
    #[cfg_attr(feature = "schema", schemars(with = "Option<Vec<FrameAxis>>"))]
    pub linear_axes: Option<BTreeSet<FrameAxis>>,
    /// Whether the machine controls rotary axes.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rotary_axes"
    )]
    pub rotary_axes: Option<RotaryAxes>,
    /// The machine's tool table.
    pub pockets: Vec<ToolPocket>,
    /// The source's own reference to its record.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_native_ref"
    )]
    pub native_ref: Option<String>,
}

/// Whether a machine controls rotary axes. It states neither their
/// directions nor their kinematic order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RotaryAxes {
    /// No rotary axes: the tool axis is fixed in the machine's frame.
    Absent {},
    /// Rotary axes.
    Present {
        /// The number of rotary axes, where the source states it.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_count"
        )]
        count: Option<NonZeroU32>,
    },
}

/// A set of linear axes, read as a §2.5 set.
#[derive(Deserialize)]
#[serde(transparent)]
struct AxisSet(#[serde(with = "cadmpeg_core::distinct_keys::set")] BTreeSet<FrameAxis>);

impl From<AxisSet> for BTreeSet<FrameAxis> {
    fn from(AxisSet(axes): AxisSet) -> Self {
        axes
    }
}

/// A pocket of a machine's tool table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ToolPocket {
    /// The tool the pocket holds.
    pub tool: ToolId,
    /// The pocket's NC numbers.
    pub nc: NcNumbers,
}

cadmpeg_core::named_optional_field!(deserialize_catalog, ToolCatalog, "catalog");
cadmpeg_core::named_optional_field!(deserialize_chip_load, ProcessDimension<Length>, "chip_load");
cadmpeg_core::named_optional_field!(deserialize_control, NonBlankString, "control");
cadmpeg_core::named_optional_field!(
    deserialize_corner_radius,
    ProcessDimension<Length>,
    "corner_radius"
);
cadmpeg_core::named_optional_field!(deserialize_count, NonZeroU32, "count");
cadmpeg_core::named_optional_field!(deserialize_cross_section, char, "cross_section");
cadmpeg_core::named_optional_field!(deserialize_diameter, ProcessDimension<Length>, "diameter");
cadmpeg_core::named_optional_field!(
    deserialize_diameter_offset,
    ProcessDimension<u32>,
    "diameter_offset"
);
cadmpeg_core::named_optional_field!(
    deserialize_flute_length,
    ProcessDimension<Length>,
    "flute_length"
);
cadmpeg_core::named_optional_field!(deserialize_flutes, ProcessDimension<u32>, "flutes");
cadmpeg_core::named_optional_field!(deserialize_hand, Hand, "hand");
cadmpeg_core::named_optional_field!(deserialize_holder, HolderProfile, "holder");
cadmpeg_core::named_optional_field!(
    deserialize_inscribed_diameter,
    ProcessDimension<Length>,
    "inscribed_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_insert, TurningInsert, "insert");
cadmpeg_core::named_optional_field!(deserialize_job, JobKind, "job");
cadmpeg_core::named_optional_field!(
    deserialize_length_offset,
    ProcessDimension<u32>,
    "length_offset"
);
cadmpeg_core::named_optional_field!(deserialize_library, LibraryRef, "library");
cadmpeg_core::named_optional_field!(
    deserialize_linear_axes,
    AxisSet as BTreeSet<FrameAxis>,
    "linear_axes"
);
cadmpeg_core::named_optional_field!(deserialize_link, NonBlankString, "link");
cadmpeg_core::named_optional_field!(deserialize_material, NonBlankString, "material");
cadmpeg_core::named_optional_field!(deserialize_model, NonBlankString, "model");
cadmpeg_core::named_optional_field!(deserialize_name, String, "name");
cadmpeg_core::named_optional_field!(deserialize_native_ref, String, "native_ref");
cadmpeg_core::named_optional_field!(deserialize_number, ProcessDimension<u32>, "number");
cadmpeg_core::named_optional_field!(
    deserialize_overall_length,
    ProcessDimension<Length>,
    "overall_length"
);
cadmpeg_core::named_optional_field!(deserialize_product_id, NonBlankString, "product_id");
cadmpeg_core::named_optional_field!(deserialize_relief, char, "relief");
cadmpeg_core::named_optional_field!(
    deserialize_relief_angle,
    ProcessDimension<Angle>,
    "relief_angle"
);
cadmpeg_core::named_optional_field!(deserialize_rotary_axes, RotaryAxes, "rotary_axes");
cadmpeg_core::named_optional_field!(
    deserialize_shank_diameter,
    ProcessDimension<Length>,
    "shank_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_shape, char, "shape");
cadmpeg_core::named_optional_field!(
    deserialize_taper_angle,
    ProcessDimension<Angle>,
    "taper_angle"
);
cadmpeg_core::named_optional_field!(deserialize_thickness, ProcessDimension<Length>, "thickness");
cadmpeg_core::named_optional_field!(
    deserialize_thread_pitch,
    ProcessDimension<Length>,
    "thread_pitch"
);
cadmpeg_core::named_optional_field!(deserialize_tip_angle, ProcessDimension<Angle>, "tip_angle");
cadmpeg_core::named_optional_field!(
    deserialize_tip_diameter,
    ProcessDimension<Length>,
    "tip_diameter"
);
cadmpeg_core::named_optional_field!(deserialize_tolerance, char, "tolerance");
cadmpeg_core::named_optional_field!(deserialize_turret, ProcessDimension<u32>, "turret");
cadmpeg_core::named_optional_field!(deserialize_unit, UnitSymbol, "unit");
cadmpeg_core::named_optional_field!(deserialize_vendor, NonBlankString, "vendor");
cadmpeg_core::named_optional_field!(deserialize_width, ProcessDimension<Length>, "width");
