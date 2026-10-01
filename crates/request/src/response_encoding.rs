use std::io::{self, Write};

use flate2::{Decompress, FlushDecompress, Status, write::MultiGzDecoder};
use http_client::http::{HeaderMap, header::CONTENT_ENCODING};

use crate::ExecutionError;

/// The content codings advertised to servers, in order of preference.
pub(crate) const ACCEPTED_CODINGS: &str = "gzip, deflate, br, zstd";

/// Encoded bytes go to a decoder this many at a time, so the size limit stops
/// a highly compressed body soon after it is exceeded.
const CHUNK: usize = 16 * 1024;

/// A content coding that responses are decoded from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Coding {
    Gzip,
    /// A zlib stream, or the raw deflate stream some servers send instead.
    Deflate,
    Brotli,
    Zstd,
}

/// The codings applied to a body, in the order they were applied, or `None`
/// when one of them cannot be decoded. Identity codings are left out.
pub(crate) fn codings(headers: &HeaderMap) -> Option<Vec<Coding>> {
    let mut codings = Vec::new();

    for value in headers.get_all(CONTENT_ENCODING) {
        for name in value.to_str().ok()?.split(',').map(str::trim) {
            let coding = match name.to_ascii_lowercase().as_str() {
                "" | "identity" => continue,
                "gzip" | "x-gzip" => Coding::Gzip,
                "deflate" => Coding::Deflate,
                "br" => Coding::Brotli,
                "zstd" => Coding::Zstd,
                _ => return None,
            };
            codings.push(coding);
        }
    }

    Some(codings)
}

pub(crate) async fn decode_body(
    headers: &HeaderMap,
    mut body: Vec<u8>,
    limit_bytes: Option<u64>,
) -> Result<(Vec<u8>, Option<usize>), ExecutionError> {
    // Preserve unsupported encodings as received, including mixed stacks.
    // Partially decoding a stack would leave the displayed bytes ambiguous.
    let Some(codings) = codings(headers).filter(|codings| !codings.is_empty()) else {
        return Ok((body, None));
    };

    // CPU-heavy decompression must yield to the executor so the total request
    // deadline can fire while this work runs on the blocking pool.
    smol::unblock(move || {
        let encoded_bytes = body.len();

        // The last coding applied is the first to undo.
        for coding in codings.into_iter().rev() {
            let mut decoder = Decoder::new(Some(coding), limit_bytes);

            for chunk in body.chunks(CHUNK) {
                decoder.decode(chunk)?;
            }
            decoder.finish()?;
            body = decoder.into_decoded();
        }

        Ok((body, Some(encoded_bytes)))
    })
    .await
}

/// Undoes one content coding as the encoded bytes arrive. The decoded bytes
/// collect in order; more than the limit fails the response.
pub(crate) struct Decoder(Layer);

enum Layer {
    Identity(Output),
    Gzip(Box<MultiGzDecoder<Output>>),
    Deflate(Box<Inflate>),
    Brotli(Box<brotli_decompressor::DecompressorWriter<Output>>),
    Zstd(Box<zstd::stream::zio::Writer<Output, zstd::stream::raw::Decoder<'static>>>),
}

impl Decoder {
    pub(crate) fn new(coding: Option<Coding>, limit_bytes: Option<u64>) -> Self {
        let output = Output {
            decoded: Vec::new(),
            limit_bytes,
            exceeded: false,
        };

        Self(match coding {
            None => Layer::Identity(output),
            Some(Coding::Gzip) => Layer::Gzip(Box::new(MultiGzDecoder::new(output))),
            Some(Coding::Deflate) => Layer::Deflate(Box::new(Inflate {
                decompress: None,
                start: Vec::new(),
                ended: false,
                output,
            })),
            Some(Coding::Brotli) => Layer::Brotli(Box::new(
                brotli_decompressor::DecompressorWriter::new(output, CHUNK),
            )),
            Some(Coding::Zstd) => Layer::Zstd(Box::new(zstd::stream::zio::Writer::new(
                output,
                zstd::stream::raw::Decoder::new().expect("zstd decoder"),
            ))),
        })
    }

    pub(crate) fn is_identity(&self) -> bool {
        matches!(self.0, Layer::Identity(_))
    }

    /// Decodes the next encoded bytes. Everything they encode is decoded at
    /// once, so a stream's events are not held back.
    pub(crate) fn decode(&mut self, chunk: &[u8]) -> Result<(), ExecutionError> {
        let result = match &mut self.0 {
            Layer::Identity(output) => output.write_all(chunk),
            Layer::Gzip(decoder) => decoder.write_all(chunk).and_then(|()| decoder.flush()),
            Layer::Deflate(inflate) => inflate.write(chunk),
            Layer::Brotli(decoder) => decoder.write_all(chunk).and_then(|()| decoder.flush()),
            Layer::Zstd(decoder) => decoder.write_all(chunk).and_then(|()| decoder.flush()),
        };

        self.check(result)
    }

    /// The encoded bytes ended. Fails when they ended inside the encoding.
    pub(crate) fn finish(&mut self) -> Result<(), ExecutionError> {
        let result = match &mut self.0 {
            Layer::Identity(_) => Ok(()),
            Layer::Gzip(decoder) => decoder.try_finish(),
            Layer::Deflate(inflate) => {
                if inflate.ended {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the deflate stream ended early",
                    ))
                }
            }
            Layer::Brotli(decoder) => decoder.close(),
            Layer::Zstd(decoder) => decoder.finish(),
        };

        self.check(result)
    }

    /// Everything decoded so far.
    pub(crate) fn decoded(&self) -> &[u8] {
        &self.output().decoded
    }

    pub(crate) fn into_decoded(mut self) -> Vec<u8> {
        std::mem::take(&mut self.output_mut().decoded)
    }

    fn check(&self, result: io::Result<()>) -> Result<(), ExecutionError> {
        let output = self.output();

        match (result, output.limit_bytes) {
            (Err(_), Some(limit_bytes)) if output.exceeded => {
                Err(ExecutionError::ResponseTooLarge { limit_bytes })
            }
            (result, _) => result.map_err(ExecutionError::DecodeBody),
        }
    }

    fn output(&self) -> &Output {
        match &self.0 {
            Layer::Identity(output) => output,
            Layer::Gzip(decoder) => decoder.get_ref(),
            Layer::Deflate(inflate) => &inflate.output,
            Layer::Brotli(decoder) => decoder.get_ref(),
            Layer::Zstd(decoder) => decoder.writer(),
        }
    }

    fn output_mut(&mut self) -> &mut Output {
        match &mut self.0 {
            Layer::Identity(output) => output,
            Layer::Gzip(decoder) => decoder.get_mut(),
            Layer::Deflate(inflate) => &mut inflate.output,
            Layer::Brotli(decoder) => decoder.get_mut(),
            Layer::Zstd(decoder) => decoder.writer_mut(),
        }
    }
}

/// Collects decoded bytes, refusing those past the limit.
struct Output {
    decoded: Vec<u8>,
    limit_bytes: Option<u64>,
    exceeded: bool,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(limit_bytes) = self.limit_bytes
            && (self.decoded.len() + bytes.len()) as u64 > limit_bytes
        {
            self.exceeded = true;
            return Err(io::Error::other("the decoded body exceeds the size limit"));
        }

        self.decoded.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The `deflate` coding. It names a zlib stream, but some servers send a raw
/// deflate stream, which the first two bytes tell apart.
struct Inflate {
    decompress: Option<Decompress>,
    /// The first byte, while the second has not arrived.
    start: Vec<u8>,
    ended: bool,
    output: Output,
}

impl Inflate {
    fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        if self.ended {
            // Bytes after the end of the stream are not part of the body.
            return Ok(());
        }

        let start;
        let mut input = chunk;
        if self.decompress.is_none() {
            self.start.extend_from_slice(chunk);
            let [first, second, ..] = self.start[..] else {
                return Ok(());
            };

            // A zlib header names the deflate method with a window of at most
            // 32 KiB, and its two bytes are a multiple of 31.
            let zlib = first & 0x0f == 8
                && first >> 4 <= 7
                && (u16::from(first) << 8 | u16::from(second)) % 31 == 0;
            self.decompress = Some(Decompress::new(zlib));
            start = std::mem::take(&mut self.start);
            input = &start;
        }

        let decompress = self.decompress.as_mut().unwrap();
        let mut buffer = vec![0; CHUNK];

        loop {
            let (read, written) = (decompress.total_in(), decompress.total_out());
            let status = decompress
                .decompress(input, &mut buffer, FlushDecompress::None)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let read = (decompress.total_in() - read) as usize;
            let written = (decompress.total_out() - written) as usize;

            self.output.write_all(&buffer[..written])?;
            input = &input[read..];

            if status == Status::StreamEnd {
                self.ended = true;
                return Ok(());
            }

            // Stop when the input is used up and nothing more is pending.
            if (input.is_empty() && written < buffer.len()) || (read == 0 && written == 0) {
                return Ok(());
            }
        }
    }
}
