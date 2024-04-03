// MIT License
// 
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SampleMetadata {
    pub rate: u32,
    pub channels: u8,
    pub src_fmt_display: String,
}

pub trait SampleTrait {
    fn uri(&self) -> &str;
    fn name(&self) -> &str;
    fn metadata(&self) -> &SampleMetadata;
    fn source_uuid(&self) -> Option<&Uuid>;
}

#[derive(Debug, Clone)]
pub struct BasicSample {
    uri: String,
    name: String,
    metadata: SampleMetadata,
    source_uuid: Option<Uuid>,
}

impl BasicSample {
    pub fn new(
        uri: String,
        name: String,
        metadata: SampleMetadata,
        source_uuid: Option<Uuid>,
    ) -> BasicSample {
        BasicSample {
            uri,
            name,
            metadata,
            source_uuid,
        }
    }
}

impl SampleTrait for BasicSample {
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
#[derive(Debug, Clone)]
pub enum Sample {
    BasicSample(BasicSample),
}

impl SampleTrait for Sample {
    fn uri(&self) -> &str {
        match self {
            Self::BasicSample(s) => s.uri(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::BasicSample(s) => s.name(),
        }
    }

    fn metadata(&self) -> &SampleMetadata {
        match self {
            Self::BasicSample(s) => s.metadata(),
        }
    }

    fn source_uuid(&self) -> Option<&Uuid> {
        match self {
            Self::BasicSample(s) => s.source_uuid(),
        }
    }
}

impl From<BasicSample> for Sample {
    fn from(sample: BasicSample) -> Self {
        Sample::BasicSample(sample)
    }
}
