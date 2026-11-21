//! Leitor de CD-ROM mínimo (somente HuCard).
//!
//! Mantém a API usada por `pce-core` para que o core possa ser compilado sem
//! `@em-emu/native` (CHD/zstd). Nenhum disco é carregado pelo Emmu.

use crate::cdtime::CdTime;
use crate::cue::CueSheet;
use crate::{BYTES_PER_SECTOR, CdRomError};
use bincode::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct CdRom {
    cue_sheet: CueSheet,
}

impl Default for CdRom {
    fn default() -> Self {
        Self {
            cue_sheet: CueSheet::empty(),
        }
    }
}

impl CdRom {
    #[must_use]
    pub fn cue(&self) -> &CueSheet {
        &self.cue_sheet
    }

    /// Nenhum disco é suportado neste build; preenche o setor com zeros.
    pub fn read_sector(
        &mut self,
        _track_number: u8,
        _relative_time: CdTime,
        out: &mut [u8],
    ) -> Result<(), CdRomError> {
        out[..BYTES_PER_SECTOR as usize].fill(0);
        Ok(())
    }
}
