//! Subset de `cdrom` do JGenesis usado pelo core PCE do Emmu.
//!
//! O código original está sob GPL-3.0; veja `vendor/jgenesis/LICENSE`.

pub mod cdtime;
pub mod cue;
pub mod reader;

use std::fmt::{Display, Formatter};

// Data: 16 header bytes + 2048 data bytes + 288 error detection/correction bytes
pub const BYTES_PER_SECTOR: u64 = 2352;

#[derive(Debug)]
pub struct CdRomError(String);

impl Display for CdRomError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CdRomError {}

impl From<String> for CdRomError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

pub type CdRomResult<T> = Result<T, CdRomError>;
