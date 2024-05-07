// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

use std::ops::Deref;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SampleMetadata {
    pub rate: u32,
    pub channels: u8,
    pub src_fmt_display: String,

    // TODO: slow and/or wasteful to include these? better to fetch on request?
    pub size_bytes: Option<u64>,
    pub length_millis: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleURI(pub String);

impl Deref for SampleURI {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<str> for SampleURI {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

pub trait SampleOps {
    fn uri(&self) -> &SampleURI;
    fn name(&self) -> &str;
    fn metadata(&self) -> &SampleMetadata;
    fn source_uuid(&self) -> Option<&Uuid>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BaseSample {
    uri: SampleURI,
    name: String,
    metadata: SampleMetadata,
    source_uuid: Option<Uuid>,
}

impl BaseSample {
    pub fn new(
        uri: &SampleURI,
        name: &str,
        metadata: &SampleMetadata,
        source_uuid: Option<Uuid>,
    ) -> BaseSample {
        BaseSample {
            uri: SampleURI(uri.to_string()),
            name: name.to_string(),
            metadata: metadata.clone(),
            source_uuid,
        }
    }
}

impl SampleOps for BaseSample {
    fn uri(&self) -> &SampleURI {
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
    fn uri(&self) -> &SampleURI {
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
