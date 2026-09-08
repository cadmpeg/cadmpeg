/// Counted face and loop framing controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum B5FramingControl {
    /// Control byte `03`.
    Control03 = 0x03,
    /// Control byte `05`.
    Control05 = 0x05,
}

impl B5FramingControl {
    /// Admit a declared control byte.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x03 => Some(Self::Control03),
            0x05 => Some(Self::Control05),
            _ => None,
        }
    }

    #[cfg(test)]
    /// Native control byte.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Physical-edge terminal controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum B5EdgeTerminalControl {
    /// Control byte `01`.
    Control01 = 0x01,
    /// Control byte `02`.
    Control02 = 0x02,
    /// Control byte `21`.
    Control21 = 0x21,
    /// Control byte `22`.
    Control22 = 0x22,
    /// Control byte `25`.
    Control25 = 0x25,
    /// Control byte `26`.
    Control26 = 0x26,
    /// Control byte `29`.
    Control29 = 0x29,
    /// Control byte `2a`.
    Control2A = 0x2a,
}

impl B5EdgeTerminalControl {
    /// Admit a declared control byte.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Control01),
            0x02 => Some(Self::Control02),
            0x21 => Some(Self::Control21),
            0x22 => Some(Self::Control22),
            0x25 => Some(Self::Control25),
            0x26 => Some(Self::Control26),
            0x29 => Some(Self::Control29),
            0x2a => Some(Self::Control2A),
            _ => None,
        }
    }

    #[cfg(test)]
    /// Native control byte.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Vertex-incidence terminal controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum B5VertexIncidenceControl {
    /// Control byte `00`.
    Control00 = 0x00,
    /// Control byte `04`.
    Control04 = 0x04,
}

impl B5VertexIncidenceControl {
    /// Admit a declared control byte.
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Control00),
            0x04 => Some(Self::Control04),
            _ => None,
        }
    }

    #[cfg(test)]
    /// Native control byte.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}
