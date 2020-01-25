use futures::future::{Future, FutureExt};
use futures::io::{AsyncRead, AsyncReadExt, BufReader};
use futures::stream::Stream;
use futures::task::{Context, Poll};
use std::io::{Error, ErrorKind};
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use crate::cipher::{Cipher, SharedCipher};
use crate::{Message, MAX_MESSAGE_SIZE};

type StreamState<R> = (Message, BufReader<R>, SharedCipher);

/// A reader for SMC messages.
///
/// Takes any [`futures::io::AsyncRead`] and is a
/// [`async_std::stream::Stream`] of [`Message`]s.
///
/// # Example
///
/// ```rust
/// use simple_message_channels::Reader;
/// let stdin = io::stdin().lock().await;
/// let mut reader = Reader::new(stdin);
/// while let Some(msg) = reader.next().await {
///     let msg = msg?;
///     println!("Received: ch {} typ {} msg {:?}", msg.channel, msg.typ, text);
/// }
/// ```
pub struct Reader<R> {
    future: Pin<Box<dyn Future<Output = Result<StreamState<R>, Error>> + Send>>,
    finished: bool,
    encrypted: fn(&Message, &mut Cipher),
}

impl<R> Reader<R>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    /// Create a new message reader from any [`futures::io::AsyncRead`].
    pub fn new(reader: R) -> Self {
        Self::encrypted(reader, Arc::new(RwLock::new(Cipher::empty())), |_, _| {})
    }

    pub fn encrypted(
        reader: R,
        cipher: SharedCipher,
        encrypted: fn(&Message, &mut Cipher),
    ) -> Self {
        Self {
            encrypted,
            future: decoder(BufReader::new(reader), cipher).boxed(),
            finished: false,
        }
    }
}

// Proxy to the internal BufReader and decode messages.
impl<R> Stream for Reader<R>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    type Item = Result<Message, Error>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Message, Error>>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match self.future.poll_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                match result {
                    Ok((message, reader, cipher)) => {
                        (self.encrypted)(
                            &message,
                            &mut cipher.write().expect("could not aquire lock"),
                        );
                        // Re-init the future.
                        self.future = decoder(reader, cipher).boxed();
                        Poll::Ready(Some(Ok(message)))
                    }
                    Err(error) => {
                        self.finished = true;
                        Poll::Ready(Some(Err(error)))
                    }
                }
            }
        }
    }
}

/// Decode a single message from a BufReader.
///
/// Returns either an error or both the message and the BufReader.
pub async fn decoder<'a, R>(
    mut reader: BufReader<R>,
    cipher: SharedCipher,
) -> Result<StreamState<R>, Error>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut varint: u64 = 0;
    let mut factor = 1;
    let mut headerbuf = vec![0u8; 1];
    // Read initial varint (message length).
    loop {
        reader.read_exact(&mut headerbuf).await?;
        cipher
            .write()
            .expect("could not aquire lock")
            .try_apply(&mut headerbuf);
        let byte = headerbuf[0];
        varint += (byte as u64 & 127) * factor;
        if byte < 128 {
            break;
        }
        if varint > MAX_MESSAGE_SIZE {
            return Err(Error::new(ErrorKind::InvalidInput, "Message too long"));
        }
        factor *= 128;
    }

    // Read main message.
    let mut messagebuf = vec![0u8; varint as usize];
    reader.read_exact(&mut messagebuf).await?;
    cipher
        .write()
        .expect("could not aquire lock")
        .try_apply(&mut messagebuf);
    let message = Message::from_buf(&messagebuf)?;
    Ok((message, reader, cipher))
}
