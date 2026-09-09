// SPDX-License-Identifier: Apache-2.0
//! Closed object-model discriminator values.

macro_rules! u8_discriminator {
    (
        $(#[$meta:meta])* $vis:vis $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $value:literal,)+
        }
        $error:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "u8", into = "u8")]
        #[repr(u8)]
        $vis enum $name {
            $($(#[$variant_meta])* $variant = $value,)+
        }

        impl From<$name> for u8 {
            fn from(value: $name) -> Self { value as Self }
        }

        impl TryFrom<u8> for $name {
            type Error = &'static str;
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err($error),
                }
            }
        }
    };
    (
        $(#[$meta:meta])* $vis:vis $name:ident {
            $($(#[$variant_meta:meta])* $variant:ident = $value:literal,)+
        }
        $error:literal; ALL
    ) => {
        $crate::om::discriminators::u8_discriminator! {
            $(#[$meta])* $vis $name {
                $($(#[$variant_meta])* $variant = $value,)+
            }
            $error
        }
        impl $name {
            /// Every slot in serialized order.
            pub(crate) const ALL: [Self; [$(stringify!($variant)),+].len()] = [$(Self::$variant),+];
        }
    };
}

pub(crate) use u8_discriminator;

u8_discriminator! {
    /// Admitted `row_kind` values for an operation-state row.
    pub OperationStateCounterKind {
        /// Serialized `0x01` form.
        Form01 = 0x01,
        /// Serialized `0x02` form.
        Form02 = 0x02,
    }
    "OperationStateCounterKind.row_kind is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted `tag` values for an operation-state row.
    pub OperationStatePairTag {
        /// Serialized `0x4f` form.
        Form4f = 0x4f,
        /// Serialized `0x48` form.
        Form48 = 0x48,
    }
    "OperationStatePairTag.tag is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted branch values for `DraftIdentityBranch`.
    pub DraftIdentityBranch {
        /// Serialized `0x02` form.
        Form02 = 2,
        /// Serialized `0x03` form.
        Form03 = 3,
    }
    "DraftIdentityBranch.branch is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted branch values for `DraftBinary32Branch`.
    pub DraftBinary32Branch {
        /// Serialized `0x03` form.
        Form03 = 3,
        /// Serialized `0x04` form.
        Form04 = 4,
    }
    "DraftBinary32Branch.branch is not an admitted discriminator"
}

impl DraftBinary32Branch {
    /// Complete serialized discriminator for this branch.
    pub fn discriminator(self) -> [u8; 18] {
        match self {
            Self::Form04 => [
                0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86,
                0x02, 0x00, 0x03, 0x00,
            ],
            Self::Form03 => [
                0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
                0x02, 0x00, 0x03, 0x00,
            ],
        }
    }
}

u8_discriminator! {
    /// Admitted mode values for a surface construction branch.
    pub SurfaceBranchMode {
        /// Serialized `0x16` form.
        Form16 = 0x16,
        /// Serialized `0x40` form.
        Form40 = 0x40,
    }
    "SurfaceBranchMode.mode is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted discriminator values for an index row.
    pub LinkedIndexDiscriminator {
        /// Serialized `0x16` form.
        Form16 = 0x16,
        /// Serialized `0x17` form.
        Form17 = 0x17,
        /// Serialized `0x18` form.
        Form18 = 0x18,
    }
    "LinkedIndexDiscriminator.discriminator is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted flag values for an index row.
    pub LinkedIndexFlag {
        /// Serialized `0x03` form.
        Form03 = 0x03,
        /// Serialized `0x07` form.
        Form07 = 0x07,
    }
    "LinkedIndexFlag.flag is not an admitted discriminator"
}

u8_discriminator! {
    /// Admitted mode values for an index row.
    pub IndexRowMode {
        /// Serialized `0x04` form.
        Form04 = 0x04,
        /// Serialized `0x07` form.
        Form07 = 0x07,
    }
    "IndexRowMode.mode is not an admitted discriminator"
}

impl super::scalar_run::ScalarFrame for DraftBinary32Branch {
    type Atom = super::scalar::ShiftedBinary32;
    fn prefix_len(self) -> u64 {
        self.discriminator().len() as u64
    }
}

u8_discriminator! {
    /// Branch introducing an operation body reference lane.
    pub OperationBodyReferenceBranch {
        Form11 = 0x11,
        Form1c = 0x1c,
    }
    "OperationBodyReferenceBranch.branch is not an admitted discriminator"
}

/// Admitted modes of a point construction header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "u8", into = "u8")]
#[repr(u8)]
pub enum PointHeaderMode {
    /// Serialized `0x02` form.
    Form02 = 0x02,
    /// Serialized `0x03` form.
    Form03 = 0x03,
}

impl From<PointHeaderMode> for u8 {
    fn from(mode: PointHeaderMode) -> Self {
        mode as Self
    }
}

impl TryFrom<u8> for PointHeaderMode {
    type Error = &'static str;

    fn try_from(mode: u8) -> Result<Self, Self::Error> {
        match mode {
            0x02 => Ok(Self::Form02),
            0x03 => Ok(Self::Form03),
            _ => Err("PointHeaderMode.mode is not an admitted discriminator"),
        }
    }
}
