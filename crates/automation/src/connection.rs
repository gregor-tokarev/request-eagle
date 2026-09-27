use crate::MAX_MESSAGE_BYTES;
use smol::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::unix::UnixStream,
};
use std::io;

const RECORD_BYTES: usize = 60 * 1024;
const NOISE_MESSAGE_BYTES: usize = u16::MAX as usize;

/// Noise authenticates both peers with the approved session key before any app
/// command is sent. Fresh ephemeral keys and ordered AEAD records protect both
/// directions against counterfeit sockets, relays, tampering and replay.
pub struct Connection {
    stream: UnixStream,
    transport: snow::TransportState,
}

impl Connection {
    pub async fn client(stream: UnixStream, token: &str) -> io::Result<Self> {
        Self::handshake(stream, token, true).await
    }

    pub async fn server(stream: UnixStream, token: &str) -> io::Result<Self> {
        Self::handshake(stream, token, false).await
    }

    async fn handshake(mut stream: UnixStream, token: &str, initiator: bool) -> io::Result<Self> {
        let key = session_key(token)?;
        let builder = snow::Builder::new(
            "Noise_NNpsk0_25519_ChaChaPoly_SHA256"
                .parse()
                .map_err(io::Error::other)?,
        )
        .prologue(b"Request Eagle CLI protocol 1")
        .map_err(io::Error::other)?
        .psk(0, &key)
        .map_err(io::Error::other)?;
        let mut handshake = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(io::Error::other)?;
        let mut buffer = [0u8; 96];
        let result = async {
            if !initiator {
                handshake
                    .read_message(&read_record(&mut stream).await?, &mut buffer)
                    .map_err(io::Error::other)?;
            }
            let length = handshake
                .write_message(&[], &mut buffer)
                .map_err(io::Error::other)?;
            write_record(&mut stream, &buffer[..length]).await?;
            if initiator {
                handshake
                    .read_message(&read_record(&mut stream).await?, &mut buffer)
                    .map_err(io::Error::other)?;
            }
            handshake.into_transport_mode().map_err(io::Error::other)
        }
        .await;
        let transport = result.map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied,
            "Could not authenticate this app. Enable CLI access in General settings and copy the session command"))?;
        Ok(Self { stream, transport })
    }

    pub async fn send(&mut self, message: &[u8]) -> io::Result<()> {
        if message.len() as u64 > MAX_MESSAGE_BYTES {
            return Err(io::Error::other("Message exceeds the size limit"));
        }
        let mut buffer = vec![0u8; NOISE_MESSAGE_BYTES];
        // An encrypted empty record terminates the message; truncation cannot
        // turn a partial payload into a successful response.
        for chunk in message.chunks(RECORD_BYTES).chain(std::iter::once(&[][..])) {
            let length = self
                .transport
                .write_message(chunk, &mut buffer)
                .map_err(io::Error::other)?;
            write_record(&mut self.stream, &buffer[..length]).await?;
        }
        Ok(())
    }

    pub async fn receive(&mut self) -> io::Result<Vec<u8>> {
        let mut message = Vec::new();
        let mut buffer = vec![0u8; NOISE_MESSAGE_BYTES];
        loop {
            let record = read_record(&mut self.stream).await?;
            let length = self
                .transport
                .read_message(&record, &mut buffer)
                .map_err(io::Error::other)?;
            if length == 0 {
                return Ok(message);
            }
            if (message.len() + length) as u64 > MAX_MESSAGE_BYTES {
                return Err(io::Error::other("Message exceeds the size limit"));
            }
            message.extend_from_slice(&buffer[..length]);
        }
    }
}

fn session_key(token: &str) -> io::Result<[u8; 32]> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Set REQUEST_EAGLE_CLI_TOKEN using Copy session command in General settings",
        ));
    }
    let mut key = [0u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte =
            u8::from_str_radix(&token[index * 2..index * 2 + 2], 16).map_err(io::Error::other)?;
    }
    Ok(key)
}

async fn read_record(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut length = [0u8; 2];
    stream.read_exact(&mut length).await?;
    let mut record = vec![0u8; u16::from_be_bytes(length) as usize];
    stream.read_exact(&mut record).await?;
    Ok(record)
}

async fn write_record(stream: &mut UnixStream, record: &[u8]) -> io::Result<()> {
    let length = u16::try_from(record.len()).map_err(io::Error::other)?;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(record).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    async fn pair() -> (Connection, Connection) {
        let (client, server) = UnixStream::pair().unwrap();
        let (client, server) = smol::future::zip(
            Connection::client(client, TOKEN),
            Connection::server(server, TOKEN),
        )
        .await;
        (client.unwrap(), server.unwrap())
    }

    #[test]
    fn authenticated_channel_preserves_large_messages_in_both_directions() {
        smol::block_on(async {
            let (mut client, mut server) = pair().await;
            let message = vec![42; 512 * 1024];
            let (sent, received) = smol::future::zip(client.send(&message), server.receive()).await;
            sent.unwrap();
            assert_eq!(received.unwrap(), message);
            let (sent, received) =
                smol::future::zip(server.send(b"response"), client.receive()).await;
            sent.unwrap();
            assert_eq!(received.unwrap(), b"response");
        });
    }

    #[test]
    fn refuses_a_peer_without_the_approved_session_key() {
        smol::block_on(async {
            let (client, server) = UnixStream::pair().unwrap();
            let wrong = "1".repeat(64);
            let (client, server) = smol::future::zip(
                Connection::client(client, TOKEN),
                Connection::server(server, &wrong),
            )
            .await;
            assert!(client.is_err());
            assert!(server.is_err());
        });
    }

    #[test]
    fn rejects_tampered_replayed_and_cross_connection_records() {
        smol::block_on(async {
            let (mut client, mut server) = pair().await;
            let mut record = [0u8; 128];
            let length = client
                .transport
                .write_message(b"secret", &mut record)
                .unwrap();
            let mut payload = [0u8; 128];
            let mut tampered = record;
            tampered[0] ^= 1;
            assert!(
                server
                    .transport
                    .read_message(&tampered[..length], &mut payload)
                    .is_err()
            );
            assert_eq!(
                server
                    .transport
                    .read_message(&record[..length], &mut payload)
                    .unwrap(),
                6
            );
            assert!(
                server
                    .transport
                    .read_message(&record[..length], &mut payload)
                    .is_err()
            );
            let (_, mut unrelated) = pair().await;
            assert!(
                unrelated
                    .transport
                    .read_message(&record[..length], &mut payload)
                    .is_err()
            );
        });
    }
}
