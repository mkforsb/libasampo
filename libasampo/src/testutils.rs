// MIT License
//
// Copyright (c) 2024 Mikael Forsberg (github.com/mkforsb)

pub(crate) mod audiohash_for_test {
    use std::cell::Cell;

    use crate::{errors::Error, sources::SourceReader};

    thread_local! {
        pub(crate) static RESULT: Cell<Option<fn(SourceReader) -> Result<String, Error>>>
            = Cell::new(None);
    }

    pub(crate) fn audio_hash(reader: SourceReader) -> Result<String, Error> {
        RESULT
            .get()
            .expect("A function pointer should be placed in RESULT")(reader)
    }
}

pub(crate) fn sample_from_json(json: &json::JsonValue) -> crate::samples::Sample {
    use core::panic;

    let uri = match &json["uri"] {
        json::JsonValue::Short(s) => s.to_string(),
        json::JsonValue::String(s) => s.to_string(),
        json::JsonValue::Null => "default_uri".to_string(),
        _ => panic!("sample_from_json: invalid value for `uri` (valid: String)"),
    };

    let name = match &json["name"] {
        json::JsonValue::Short(s) => s.to_string(),
        json::JsonValue::String(s) => s.to_string(),
        json::JsonValue::Null => "default_name".to_string(),
        _ => panic!("sample_from_json: invalid value for `name` (valid: String)"),
    };

    let rate = match &json["rate"] {
        json::JsonValue::Number(n) => n.as_fixed_point_u64(0).unwrap() as u32,
        json::JsonValue::Null => 44100,
        _ => panic!("sample_from_json: invalid value for `rate` (valid: Number)"),
    };

    let channels = match &json["channels"] {
        json::JsonValue::Number(n) => n.as_fixed_point_u64(0).unwrap() as u8,
        json::JsonValue::Null => 2,
        _ => panic!("sample_from_json: invalid value for `channels` (valid: Number)"),
    };

    let src_fmt_display = match &json["src_fmt_display"] {
        json::JsonValue::Short(s) => s.to_string(),
        json::JsonValue::String(s) => s.to_string(),
        json::JsonValue::Null => "default_src_fmt_display".to_string(),
        _ => panic!("sample_from_json: invalid value for `src_fmt_display` (valid: String)"),
    };

    let source_uuid = match &json["source_uuid"] {
        json::JsonValue::Short(s) => Some(uuid::Uuid::parse_str(s.to_string().as_str()).unwrap()),
        json::JsonValue::String(s) => Some(uuid::Uuid::parse_str(s.to_string().as_str()).unwrap()),
        json::JsonValue::Null => None,
        _ => panic!("sample_from_json: invalid value for `source_uuid` (valid: String)"),
    };

    crate::samples::Sample::BaseSample(crate::samples::BaseSample::new(
        &SampleURI(uri),
        &name,
        &crate::samples::SampleMetadata {
            rate,
            channels,
            src_fmt_display,
        },
        source_uuid,
    ))
}

macro_rules! sample {
    () => {
        sample!(
            json = r#"{
                "uri": "default_uri",
                "name": "default_name",
                "rate": 44100,
                "channels": 2,
                "src_fmt_display": "default_src_fmt_display"
            }"#
        )
    };

    (json=$json:expr) => {
        sample_from_json(&json::parse($json).unwrap())
    };
}

pub(crate) use sample;

pub(crate) fn fakesource_from_json(json: &json::JsonValue) -> crate::sources::Source {
    use std::collections::HashMap;

    use crate::prelude::SampleOps;

    let name = match &json["name"] {
        json::JsonValue::Boolean(b) => match b {
            true => {
                panic!("fakesource_from_json: invalid value for `name` (valid: String or false)")
            }
            false => None,
        },
        json::JsonValue::Short(s) => Some(s.to_string()),
        json::JsonValue::String(s) => Some(s.to_string()),
        json::JsonValue::Null => Some("default_name".to_string()),
        _ => panic!("fakesource_from_json: invalid value for `name` (valid: String or false)"),
    };

    let uri = match &json["uri"] {
        json::JsonValue::Short(s) => s.to_string(),
        json::JsonValue::String(s) => s.to_string(),
        json::JsonValue::Null => "default_uri".to_string(),
        _ => panic!("fakesource_from_json: invalid value for `uri` (valid: String)"),
    };

    let uuid = match &json["uuid"] {
        json::JsonValue::Short(s) => uuid::Uuid::parse_str(s.to_string().as_str())
            .expect("Should have been given a valid UUID string"),
        json::JsonValue::String(s) => uuid::Uuid::parse_str(s.to_string().as_str())
            .expect("Should have been given a valid UUID string"),
        json::JsonValue::Null => uuid::Uuid::new_v4(),
        _ => panic!("fakesource_from_json: invalid value for `uuid` (valid: String)"),
    };

    let list = match &json["list"] {
        json::JsonValue::Array(arr)
            if arr.iter().all(|x| matches!(x, json::JsonValue::Object(_))) =>
        {
            arr.iter().map(sample_from_json).collect()
        }
        json::JsonValue::Null => vec![],
        _ => panic!("fakesource_from_json:: invalid value for `list` (valid: [Sample*])"),
    };

    let all_nums = |x: &json::Array| x.iter().all(|y| matches!(y, json::JsonValue::Number(_)));

    let valid_stream_entries = |obj: &json::object::Object| {
        obj.iter()
            .all(|(_key, val)| matches!(val, json::JsonValue::Array(vals) if all_nums(vals)))
    };

    let stream = match &json["stream"] {
        json::JsonValue::Object(obj) if valid_stream_entries(obj) => obj
            .iter()
            .map(|(key, val)| match val {
                json::JsonValue::Array(vals) => (
                    SampleURI(key.to_string()),
                    vals.iter()
                        .map(|val| match val {
                            json::JsonValue::Number(num) => {
                                let (positive, mantissa, exponent) = num.as_parts();

                                if positive {
                                    (mantissa as f32).powi(exponent.into())
                                } else {
                                    (-(mantissa as f32)).powi(exponent.into())
                                }
                            }
                            _ => panic!(),
                        })
                        .collect(),
                ),
                _ => panic!(),
            })
            .collect(),
        json::JsonValue::Null if list.is_empty() => HashMap::new(),
        json::JsonValue::Null => list
            .iter()
            .map(|sample| (sample.uri().clone(), vec![]))
            .collect(),
        _ => panic!(),
    };

    let enabled = match &json["enabled"] {
        json::JsonValue::Boolean(b) => *b,
        json::JsonValue::Null => true,
        _ => panic!("fakesource_from_json: invalid value for `enabled` (valid: bool)"),
    };

    crate::sources::Source::FakeSource(crate::sources::FakeSource {
        name,
        uri,
        uuid,
        list,
        list_error: None,
        stream,
        stream_error: None,
        enabled,
    })
}

macro_rules! fakesource {
    () => {
        fakesource!(
            json = r#"{
                "name": "default_name",
                "uri": "default_uri",
                "list": [],
                "stream": {},
                "enabled": true
            }"#
        )
    };

    (json=$json:expr) => {
        fakesource_from_json(&json::parse($json).unwrap())
    };
}

pub(crate) use fakesource;

use crate::samples::SampleURI;

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::prelude::{SampleOps, SourceOps};

    use super::*;

    #[test]
    fn test_sample_from_json() {
        assert_eq!(sample!(json = r#"{ "uri": "abc123" }"#).uri(), "abc123");
        assert_eq!(sample!(json = r#"{ "name": "456xyz" }"#).name(), "456xyz");

        let sample = sample!(
            json = r#"{
                "rate": 12345,
                "channels": 42,
                "src_fmt_display": "use the fourcc oggi-wav",
                "source_uuid": "12345678-9012-3456-7890-123456789012"
            }"#
        );

        assert_eq!(sample.metadata().rate, 12345);
        assert_eq!(sample.metadata().channels, 42);
        assert_eq!(sample.metadata().src_fmt_display, "use the fourcc oggi-wav");

        assert_eq!(
            sample.source_uuid(),
            Some(uuid::Uuid::parse_str("12345678-9012-3456-7890-123456789012").unwrap()).as_ref()
        );
    }

    #[test]
    #[should_panic]
    fn test_sample_from_json_value_error() {
        sample!(json = r#"{"rate": []}"#);
    }

    #[test]
    fn test_fakesource_from_json_basics() {
        assert!(fakesource!().name().is_some());
        assert_eq!(fakesource!(json = r#"{"name": false}"#).name(), None);
        assert_eq!(fakesource!(json = r#"{"name": "x"}"#).name(), Some("x"));

        assert_eq!(fakesource!(json = r#"{"uri": "y"}"#).uri(), "y");

        assert_eq!(
            fakesource!(json = r#"{"uuid": "10000000-2000-3000-4000-500000000000"}"#).uuid(),
            &uuid::Uuid::parse_str("10000000-2000-3000-4000-500000000000").unwrap()
        );
    }

    #[test]
    fn test_fakesource_from_json_samples_stream_manual() {
        assert!(fakesource!(json = r#"{ "list": [] }"#)
            .list()
            .unwrap()
            .is_empty());

        let source = fakesource!(
            json = r#"{
            "list": [{"uri": "1.wav", "rate": 999}, {"uri": "2.wav"}],
            "stream": {"1.wav": [1,1,1], "2.wav": [2,2,2,2]}
        }"#
        );

        assert_eq!(source.list().unwrap().len(), 2);
        assert_eq!(source.list().unwrap().first().unwrap().uri(), "1.wav");
        assert_eq!(source.list().unwrap().get(1).unwrap().uri(), "2.wav");

        assert!(source
            .stream(source.list().unwrap().first().unwrap())
            .is_ok());

        assert_eq!(
            source
                .stream(source.list().unwrap().get(1).unwrap())
                .unwrap()
                .read_to_end(&mut Vec::new())
                .unwrap(),
            16
        );
    }

    #[test]
    fn test_fakesource_from_json_samples_stream_auto() {
        let source = fakesource!(
            json = r#"{
            "list": [{"uri": "1.wav", "rate": 999}, {"uri": "2.wav"}]
        }"#
        );

        assert!(source
            .stream(source.list().unwrap().first().unwrap())
            .is_ok());

        assert!(source
            .stream(source.list().unwrap().get(1).unwrap())
            .is_ok());
    }
}
