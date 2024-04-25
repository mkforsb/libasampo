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
