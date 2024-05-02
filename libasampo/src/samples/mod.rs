// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SampleMetadata {
    pub rate: u32,
    pub channels: u8,
    pub src_fmt_display: String,
}

pub trait SampleOps {
    fn uri(&self) -> &str;
    fn name(&self) -> &str;
    fn metadata(&self) -> &SampleMetadata;
    fn source_uuid(&self) -> Option<&Uuid>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseSample {
    uri: String,
    name: String,
    metadata: SampleMetadata,
    source_uuid: Option<Uuid>,
}

impl BaseSample {
    pub fn new(
        uri: String,
        name: String,
        metadata: SampleMetadata,
        source_uuid: Option<Uuid>,
    ) -> BaseSample {
        BaseSample {
            uri,
            name,
            metadata,
            source_uuid,
        }
    }
}

impl SampleOps for BaseSample {
    fn uri(&self) -> &str {
        &self.uri
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn metadata(&self) -> &SampleMetadata {
        &self.metadata
    }

    fn source_uuid(&self) -> Option<&Uuid> {
        self.source_uuid.as_ref()
    }
}

// TODO: use enum-dispatch
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Sample {
    BaseSample(BaseSample),

    #[default]
    DefaultSample,
}

impl SampleOps for Sample {
    fn uri(&self) -> &str {
        match self {
            Self::BaseSample(s) => s.uri(),
            Self::DefaultSample => panic!("Cannot call methods on DefaultSample"),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::BaseSample(s) => s.name(),
            Self::DefaultSample => panic!("Cannot call methods on DefaultSample"),
        }
    }

    fn metadata(&self) -> &SampleMetadata {
        match self {
            Self::BaseSample(s) => s.metadata(),
            Self::DefaultSample => panic!("Cannot call methods on DefaultSample"),
        }
    }

    fn source_uuid(&self) -> Option<&Uuid> {
        match self {
            Self::BaseSample(s) => s.source_uuid(),
            Self::DefaultSample => panic!("Cannot call methods on DefaultSample"),
        }
    }
}

impl From<BaseSample> for Sample {
    fn from(sample: BaseSample) -> Self {
        Sample::BaseSample(sample)
    }
}
