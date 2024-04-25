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

    let fmt = match &json["fmt"] {
        json::JsonValue::Short(s) => s.to_string(),
        json::JsonValue::String(s) => s.to_string(),
        json::JsonValue::Null => "default_fmt".to_string(),
        _ => panic!("sample_from_json: invalid value for `fmt` (valid: String)"),
    };

    let srcuuid = match &json["srcuuid"] {
        json::JsonValue::Short(s) => uuid::Uuid::parse_str(s.to_string().as_str()).unwrap(),
        json::JsonValue::String(s) => uuid::Uuid::parse_str(s.to_string().as_str()).unwrap(),
        json::JsonValue::Null => uuid::uuid!("00000000-0000-0000-0000-000000000000"),
        _ => panic!("sample_from_json: invalid value for `srcuuid` (valid: String)"),
    };

    crate::samples::Sample::BasicSample(crate::samples::BasicSample::new(
        uri,
        name,
        crate::samples::SampleMetadata {
            rate,
            channels,
            src_fmt_display: fmt,
        },
        Some(srcuuid),
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
                "fmt": "default_fmt",
                "srcuuid": "00000000-0000-0000-0000-000000000000"
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

    use crate::prelude::SampleTrait;

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
        json::JsonValue::Short(s) => uuid::Uuid::parse_str(s.to_string().as_str()).unwrap(),
        json::JsonValue::String(s) => uuid::Uuid::parse_str(s.to_string().as_str()).unwrap(),
        json::JsonValue::Null => uuid::uuid!("00000000-0000-0000-0000-000000000000"),
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

    let valid_stream_entry = |x: &json::JsonValue| match x {
        json::JsonValue::Array(y) => {
            y.len() == 2
                && match (&y[0], &y[1]) {
                    (json::JsonValue::Short(_), json::JsonValue::Array(vals)) if all_nums(vals) => {
                        true
                    }
                    (json::JsonValue::String(_), json::JsonValue::Array(vals))
                        if all_nums(vals) =>
                    {
                        true
                    }
                    _ => false,
                }
        }
        _ => false,
    };

    let stream = match &json["stream"] {
        json::JsonValue::Array(arr) if arr.iter().all(valid_stream_entry) => arr
            .iter()
            .map(|x| match x {
                json::JsonValue::Array(entry) => (
                    entry[0].to_string(),
                    match &entry[1] {
                        json::JsonValue::Array(vals) => vals
                            .iter()
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
                        _ => panic!(),
                    },
                ),
                _ => panic!(),
            })
            .collect(),
        json::JsonValue::Null => match &json["list"] {
            json::JsonValue::Null => HashMap::new(),
            _ => list
                .iter()
                .map(|sample| (sample.uri().to_string(), vec![]))
                .collect(),
        },
        _ => panic!(
            "fakesource_from_json: invalid value for `stream` (valid: [[String, [Number*]]*])"
        ),
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
                "uuid": "00000000-0000-0000-0000-000000000000",
                "list": [],
                "stream": [],
                "enabled": true
            }"#
        )
    };

    (json=$json:expr) => {
        fakesource_from_json(&json::parse($json).unwrap())
    };
}

pub(crate) use fakesource;

// TODO: test the json testutils a bit, partly as form of documentation
