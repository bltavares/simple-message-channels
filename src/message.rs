use async_std::io::Error;

#[derive(Debug)]
pub struct Message {
    pub channel: u64,
    pub typ: u8,
    pub message: Vec<u8>,
}

impl Message {
    pub fn new(channel: u64, typ: u8, message: Vec<u8>) -> Message {
        Message {
            channel,
            typ,
            message,
        }
    }

    pub fn from_buf(buf: &[u8]) -> Result<Message, Error> {
        decode_message(buf)
    }

    pub fn encode(&self) -> Vec<u8> {
        encode_message(self)
    }
}

/// Decode a message from `buf` (bytes).
///
/// Note: `buf` has to have a valid length, and the length prefixed
/// has to be removed already.
pub fn decode_message(buf: &[u8]) -> Result<Message, Error> {
    let mut header = 0 as u64;
    let headerlen = varinteger::decode(buf, &mut header);
    let msg = &buf[headerlen..];
    let channel = header >> 4;
    let typ = header & 0b1111;
    let message = Message {
        channel: channel,
        typ: typ as u8,
        message: msg.to_vec(),
    };
    Ok(message)
}

/// Encode a message body into a buffer.
pub fn encode_message(msg: &Message) -> Vec<u8> {
    let header = msg.channel << 4 | msg.typ as u64;
    let len_header = varinteger::length(header);
    let len_body = msg.message.len() + len_header;
    let len_prefix = varinteger::length(len_body as u64);

    let mut buf = vec![0; len_body + len_prefix];

    varinteger::encode(len_body as u64, &mut buf[..len_prefix]);
    let end = len_prefix + len_header;
    varinteger::encode(header, &mut buf[len_prefix..end]);
    &mut buf[end..].copy_from_slice(&msg.message);
    buf
}
