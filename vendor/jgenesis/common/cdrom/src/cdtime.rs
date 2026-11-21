//! [`CdTime`] type for more ergonomic handling of CD-ROM times and addresses.
//!
//! Este arquivo é do JGenesis (GPL-3.0), vendorizado sem alterações de lógica.

use bincode::{Decode, Encode};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct CdTime {
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
}

impl CdTime {
    pub const ZERO: Self = Self {
        minutes: 0,
        seconds: 0,
        frames: 0,
    };
    pub const SECTOR_0_START: Self = Self {
        minutes: 0,
        seconds: 2,
        frames: 0,
    };
    pub const DISC_END: Self = Self {
        minutes: 60,
        seconds: 3,
        frames: 74,
    };

    pub const MAX_MINUTES: u8 = 80;
    pub const SECONDS_PER_MINUTE: u8 = 60;
    pub const FRAMES_PER_SECOND: u8 = 75;
    pub const MAX_SECTORS: u32 = 360000;

    #[must_use]
    pub fn new(minutes: u8, seconds: u8, frames: u8) -> Self {
        assert!(minutes < Self::MAX_MINUTES);
        assert!(seconds < Self::SECONDS_PER_MINUTE);
        assert!(frames < Self::FRAMES_PER_SECOND);
        Self {
            minutes,
            seconds,
            frames,
        }
    }

    #[must_use]
    pub fn new_checked(minutes: u8, seconds: u8, frames: u8) -> Option<Self> {
        (minutes < Self::MAX_MINUTES
            && seconds < Self::SECONDS_PER_MINUTE
            && frames < Self::FRAMES_PER_SECOND)
            .then_some(Self {
                minutes,
                seconds,
                frames,
            })
    }

    #[must_use]
    pub fn to_sector_number(self) -> u32 {
        (u32::from(Self::SECONDS_PER_MINUTE) * u32::from(self.minutes) + u32::from(self.seconds))
            * u32::from(Self::FRAMES_PER_SECOND)
            + u32::from(self.frames)
    }

    #[must_use]
    pub fn from_sector_number(sector_number: u32) -> Self {
        Self::from_sector_number_checked(sector_number)
            .unwrap_or_else(|| panic!("Invalid sector number: {sector_number}"))
    }

    #[must_use]
    pub fn from_sector_number_checked(sector_number: u32) -> Option<Self> {
        if sector_number >= Self::MAX_SECTORS {
            return None;
        }
        let frames = sector_number % u32::from(Self::FRAMES_PER_SECOND);
        let seconds = (sector_number / u32::from(Self::FRAMES_PER_SECOND))
            % u32::from(Self::SECONDS_PER_MINUTE);
        let minutes = sector_number
            / (u32::from(Self::FRAMES_PER_SECOND) * u32::from(Self::SECONDS_PER_MINUTE));
        Some(Self::new(minutes as u8, seconds as u8, frames as u8))
    }

    #[must_use]
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self::from_frames(self.to_frames().saturating_sub(rhs.to_frames()))
    }

    #[must_use]
    pub fn to_frames(self) -> u32 {
        let frames: u32 = self.frames.into();
        let seconds: u32 = self.seconds.into();
        let minutes: u32 = self.minutes.into();
        frames
            + u32::from(Self::FRAMES_PER_SECOND)
                * (seconds + u32::from(Self::SECONDS_PER_MINUTE) * minutes)
    }

    #[must_use]
    pub fn from_frames(frames: u32) -> Self {
        let minutes =
            frames / (u32::from(Self::FRAMES_PER_SECOND) * u32::from(Self::SECONDS_PER_MINUTE));
        let seconds =
            (frames / u32::from(Self::FRAMES_PER_SECOND)) % u32::from(Self::SECONDS_PER_MINUTE);
        let frames = frames % u32::from(Self::FRAMES_PER_SECOND);
        Self::new(minutes as u8, seconds as u8, frames as u8)
    }
}

impl Display for CdTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}",
            self.minutes, self.seconds, self.frames
        )
    }
}

impl FromStr for CdTime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let minutes = parts.next().and_then(|part| part.parse().ok());
        let seconds = parts.next().and_then(|part| part.parse().ok());
        let frames = parts.next().and_then(|part| part.parse().ok());
        match (minutes, seconds, frames) {
            (Some(minutes), Some(seconds), Some(frames)) => {
                Self::new_checked(minutes, seconds, frames)
                    .ok_or_else(|| format!("invalid CD time: {s}"))
            }
            _ => Err(format!("invalid CD time: {s}")),
        }
    }
}

impl Add for CdTime {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::from_frames(self.to_frames() + rhs.to_frames())
    }
}

impl AddAssign for CdTime {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for CdTime {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_frames(self.to_frames().saturating_sub(rhs.to_frames()))
    }
}

impl SubAssign for CdTime {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl PartialOrd for CdTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CdTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.to_frames().cmp(&other.to_frames())
    }
}
