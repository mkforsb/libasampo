// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use thiserror::Error as ThisError;

macro_rules! error_enum {
    ($name:ident = { $( $err:ident ),+ }) => {
        #[derive(Debug, ThisError)]
        #[allow(clippy::enum_variant_names)]
        pub enum $name {
            $(
                #[error("{0}")]
                $err(#[from] $err),
            )+
        }
    };

    ($name:ident = { $( $err:ident ),+ } where $( $dst:ident from $src:path ),+ ) => {
        #[derive(Debug, ThisError)]
        #[allow(clippy::enum_variant_names)]
        pub enum $name {
            $(
                #[error("{0}")]
                $err(#[from] $err),
            )+
        }

        $(
            impl From<$src> for $name {
                fn from(value: $src) -> $name {
                    $name::$dst(value.into())
                }
            }
        )+
    };
}

pub(crate) use error_enum;

#[derive(Debug, ThisError)]
#[error("IO error: {0}")]
pub struct IOError(#[from] std::io::Error);

#[derive(Debug, ThisError)]
#[error("Symphonia error: {0}")]
pub struct SymphoniaError(#[from] symphonia::core::errors::Error);

#[derive(Debug, ThisError)]
#[error("Symphonia source error: {0}")]
pub struct SymphoniaSourceError(pub String);

#[derive(Debug, ThisError)]
#[error("Invalid buffer size: {0}")]
pub struct InvalidBufferSizeError(pub String);

#[derive(Debug, ThisError)]
#[error("Mismatched spec")]
pub struct MismatchedSpecError;

#[derive(Debug, ThisError)]
#[error("Value out of range: {0}")]
pub struct ValueOutOfRangeError(pub String);

#[derive(Debug, ThisError)]
#[error("Channel disconnected")]
pub struct ChannelDisconnectedError;
