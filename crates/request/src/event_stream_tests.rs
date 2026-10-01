use std::time::SystemTime;

use crate::event_stream::Parser;

/// (type, data, id) of each event, after the stream arrived in `chunks`.
fn parse(chunks: &[&[u8]]) -> Vec<(String, String, String)> {
    let mut parser = Parser::default();
    let mut events = Vec::new();

    for chunk in chunks {
        parser.push(chunk, SystemTime::now(), &mut events);
    }

    events
        .into_iter()
        .map(|event| (event.event, event.data, event.id))
        .collect()
}

fn event(event: &str, data: &str, id: &str) -> (String, String, String) {
    (event.into(), data.into(), id.into())
}

#[test]
fn dispatches_events_at_blank_lines() {
    let stream = b"data: first\n\nevent: update\ndata: {\"n\":1}\nid: 7\n\ndata: tail";

    assert_eq!(
        parse(&[stream]),
        [
            event("message", "first", ""),
            event("update", "{\"n\":1}", "7"),
        ],
        "an event without its blank line is incomplete"
    );
}

#[test]
fn every_line_ending_and_chunk_boundary_reads_the_same() {
    let expected = [
        event("message", "one\ntwo", ""),
        event("ping", "", "a"),
        event("message", "three", "a"),
    ];

    for ending in ["\n", "\r", "\r\n"] {
        let stream = [
            "data: one",
            "data:two",
            "",
            "event: ping",
            "data",
            "id:a",
            "",
            "data: three",
            "",
            "",
        ]
        .join(ending);
        let bytes = stream.as_bytes();

        assert_eq!(parse(&[bytes]), expected, "{ending:?}");

        // Splitting between CR and LF must not end a second line.
        for split in 0..bytes.len() {
            assert_eq!(
                parse(&[&bytes[..split], &bytes[split..]]),
                expected,
                "{ending:?} split at {split}"
            );
        }

        let bytes: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(parse(&bytes), expected, "{ending:?} one byte at a time");
    }
}

#[test]
fn ignores_comments_unknown_fields_and_events_without_data() {
    let stream =
        b": keep-alive\n\nretry: 1000\nevent: empty\n\nfoo: bar\ndata:  two spaces\n:comment\n\n";

    assert_eq!(parse(&[stream]), [event("message", " two spaces", "")]);
}

#[test]
fn ids_persist_until_changed_and_reject_null() {
    let stream = b"id: 1\ndata: a\n\ndata: b\n\nid: 2\0x\ndata: c\n\nid\ndata: d\n\n";

    assert_eq!(
        parse(&[stream]),
        [
            event("message", "a", "1"),
            event("message", "b", "1"),
            event("message", "c", "1"),
            event("message", "d", ""),
        ]
    );
}

#[test]
fn decodes_utf8_across_chunks_and_strips_a_leading_byte_order_mark() {
    let stream = "\u{feff}data: héllo ☃\n\ndata: \u{feff}kept\n\n".as_bytes();
    let expected = [
        event("message", "héllo ☃", ""),
        event("message", "\u{feff}kept", ""),
    ];

    for split in 0..stream.len() {
        assert_eq!(parse(&[&stream[..split], &stream[split..]]), expected);
    }

    assert_eq!(
        parse(&[b"data: \xff\n\n"]),
        [event("message", "\u{fffd}", "")]
    );
}
