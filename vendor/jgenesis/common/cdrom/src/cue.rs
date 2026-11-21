//! Representação mínima de CUE/TOC usada pelo core PCE.
//!
//! O Emmu suporta apenas HuCard (ROM). O leitor de CD completa do JGenesis foi
//! removido deste vendor para evitar dependências nativas (CHD/zstd) que não
//! compilam em `wasm32`; esta versão mantém apenas os tipos necessários.

use crate::cdtime::CdTime;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TrackType {
    Data,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TrackMode {
    Mode1,
    Mode1DataOnly,
    Mode2,
    Audio,
}

impl TrackMode {
    #[must_use]
    pub fn to_type(self) -> TrackType {
        match self {
            Self::Mode1 | Self::Mode1DataOnly | Self::Mode2 => TrackType::Data,
            Self::Audio => TrackType::Audio,
        }
    }

    #[must_use]
    pub fn bytes_per_sector(self) -> u64 {
        match self {
            Self::Mode1DataOnly => 2048,
            Self::Mode1 | Self::Mode2 | Self::Audio => 2352,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Track {
    pub number: u8,
    pub mode: TrackMode,
    pub track_type: TrackType,
    pub start_time: CdTime,
    pub end_time: CdTime,
    pub pregap_len: CdTime,
    pub pause_len: CdTime,
    pub postgap_len: CdTime,
}

impl Track {
    #[must_use]
    pub fn effective_start_time(&self) -> CdTime {
        self.start_time + self.pregap_len + self.pause_len
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CueSheet {
    tracks: Vec<Track>,
    track_start_times: Vec<CdTime>,
}

impl CueSheet {
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self {
            tracks: Vec::new(),
            track_start_times: Vec::new(),
        }
    }

    #[must_use]
    pub fn track(&self, track_number: u8) -> &Track {
        &self.tracks[(track_number - 1) as usize]
    }

    pub fn tracks_iter(&self) -> impl Iterator<Item = &Track> {
        self.tracks.iter()
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn last_track(&self) -> &Track {
        self.tracks.last().expect("nenhuma faixa de CD disponível")
    }

    #[must_use]
    pub fn find_track_by_time(&self, _time: CdTime) -> Option<&Track> {
        None
    }
}
