use std::io::Read;

use flate2::read::MultiGzDecoder;
use http_client::http::{HeaderMap, header::CONTENT_ENCODING};

use crate::{ExecutionError, HttpError};

pub(crate) fn decode_body(
    headers: &HeaderMap,
    mut body: Vec<u8>,
    limit_bytes: Option<u64>,
) -> Result<(Vec<u8>, Option<usize>), ExecutionError> {
    let Some(values) = headers
        .get_all(CONTENT_ENCODING)
        .iter()
        .map(|value| value.to_str().ok())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok((body, None));
    };
    let encodings: Vec<_> = values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .collect();

    // Preserve unsupported encodings as received, including mixed stacks.
    // Partially decoding a stack would leave the displayed bytes ambiguous.
    if encodings.iter().any(|encoding| {
        !encoding.eq_ignore_ascii_case("gzip") && !encoding.eq_ignore_ascii_case("identity")
    }) {
        return Ok((body, None));
    }

    let layers = encodings
        .iter()
        .filter(|encoding| encoding.eq_ignore_ascii_case("gzip"))
        .count();

    if layers == 0 {
        return Ok((body, None));
    }

    let encoded_bytes = body.len();

    for _ in 0..layers {
        // A gzip representation may contain several concatenated members.
        let mut decoder = MultiGzDecoder::new(body.as_slice());
        let mut decoded = Vec::new();

        match limit_bytes {
            Some(limit_bytes) => {
                decoder
                    .take(limit_bytes.saturating_add(1))
                    .read_to_end(&mut decoded)
                    .map_err(HttpError::DecodeBody)?;

                if decoded.len() as u64 > limit_bytes {
                    return Err(ExecutionError::ResponseTooLarge { limit_bytes });
                }
            }
            None => {
                decoder
                    .read_to_end(&mut decoded)
                    .map_err(HttpError::DecodeBody)?;
            }
        }

        body = decoded;
    }

    Ok((body, Some(encoded_bytes)))
}
