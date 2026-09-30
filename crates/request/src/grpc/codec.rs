use prost::Message as _;
use prost_reflect::{DynamicMessage, MessageDescriptor};
use tonic::{
    Status,
    codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder},
};

/// Encodes any dynamic message and decodes messages of one type, so calls
/// work with descriptors loaded at runtime instead of generated code.
pub(crate) struct DynamicCodec {
    decode: MessageDescriptor,
}

impl DynamicCodec {
    pub(crate) fn new(decode: MessageDescriptor) -> Self {
        Self { decode }
    }
}

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder(self.decode.clone())
    }
}

pub(crate) struct DynamicEncoder;

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst)
            .map_err(|error| Status::internal(format!("could not encode the message: {error}")))
    }
}

pub(crate) struct DynamicDecoder(MessageDescriptor);

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        DynamicMessage::decode(self.0.clone(), src)
            .map(Some)
            .map_err(|error| {
                Status::internal(format!(
                    "could not decode a {} message: {error}",
                    self.0.full_name()
                ))
            })
    }
}
