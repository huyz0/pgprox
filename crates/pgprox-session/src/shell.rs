//! The I/O shell: the only part of this crate that touches a socket.
//!
//! Everything else here is a state machine. This drives them, and it is
//! deliberately thin, because every line in it is a line the sans-I/O tests
//! cannot reach.
//!
//! Generic over `AsyncRead + AsyncWrite + Unpin`, so the tests run a whole
//! session over `tokio::io::duplex` and never bind a port.
//!
//! # The buffer belongs to the connection, and the connection borrows it
//!
//! A read pulls in whatever the kernel had, which routinely includes the start
//! of the next stage's message. A helper that owned its read buffer locally
//! would drop those bytes on return and the session would appear to hang. That
//! has already happened once in this project, in the SCRAM tests, and this
//! crate's `AGENTS.md` says so at length. So [`Wire`] owns the buffer for as
//! long as there are bytes in it, and every stage borrows it.
//!
//! What it does not do is hold one while idle. At 100k connections a 16 KiB
//! read buffer and a 16 KiB write buffer per connection is 3.2 GB of memory
//! doing nothing, which is the entire reason this proxy exists. So the buffers
//! come from [`BufferSlab`] when the socket has something to say and go back
//! the moment the wire is quiet: after a flush, and after a frame that
//! consumed everything read. An idle connection costs a socket and this
//! struct.
//!
//! When the slab is empty the wire waits rather than allocating, which turns a
//! synchronised burst into latency instead of a memory spike. That is the
//! correct direction to fail and it is the whole point of the bound.
//!
//! # Cancellation
//!
//! Dropping any of these futures mid-frame is safe: bytes that arrived stay in
//! the buffer and the next call continues from where it stopped. Nothing here
//! consumes from the buffer until a whole frame is present.

use std::sync::Arc;
use std::time::Duration;

use pgprox_core::auth::{CredentialResolver, Grant};
use pgprox_core::buf::{BufferSlab, PooledBuf};
use pgprox_core::error::ClientError;
use pgprox_core::ids::ConnId;
use pgprox_proto::backend::TxStatus;
use pgprox_proto::frame::{
    Decoded, FrameHeader, LEN_PREFIX, Tag, decode, decode_header, decode_untagged,
};
use pgprox_proto::{encode, startup};
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::auth::{Progress, SCRAM_SHA_256, SaslProgress, ScramAuth, StaticCredentials, TokenAuth};
use crate::state::{Action, Credential, Handshake};

/// Why a session could not be driven.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShellError {
    /// The client went away.
    #[error("the client disconnected")]
    Disconnected,
    /// The socket failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The bytes were not a frame.
    #[error("frame: {0}")]
    Frame(#[from] pgprox_proto::frame::DecodeError),
    /// The startup packet was not one.
    #[error("startup: {0}")]
    Startup(#[from] startup::StartupError),
    /// The client was refused, and told why before the socket closed.
    #[error("refused: {0}")]
    Refused(ClientError),
}

/// What the startup phase ended in.
#[derive(Debug, PartialEq, Eq)]
pub enum Handoff {
    /// `S` was sent. Wrap the stream in TLS and negotiate again on the result.
    Upgrade,
    /// The client was asked for this credential.
    Ask(Credential),
    /// A cancellation for a key this proxy issued. Nothing else follows.
    Cancel(ConnId),
}

/// The first read of a quiet connection, on the stack.
///
/// Sized for a statement rather than for a result set: this array lives in the
/// session future, which every open connection holds, so a kilobyte here is a
/// hundred megabytes at a hundred thousand connections. Anything longer than
/// this continues into the borrowed buffer, which costs one more syscall.
const FIRST_READ: usize = 512;

/// Every read after that, into the buffer already held.
const HELD_READ: usize = 16 * 1024;

/// The largest message the proxy will read before a client has authenticated.
///
/// A client sends exactly two things before it has proved anything: a startup
/// packet and a password. Neither is large, and both were being read against
/// `DEFAULT_MAX_FRAME`, which is a gigabyte because it is sized for a `DataRow`
/// carrying a `bytea`. Nothing about a handshake resembles that. An
/// unauthenticated client could make a wire grow its buffer to whatever it was
/// willing to send, on as many connections as it could open.
///
/// Postgres does not allow this either: `MAX_STARTUP_PACKET_LENGTH` is 10000
/// bytes. This is larger, because a startup packet carrying a long `options`
/// string and a JWT with a full claim set both have to fit and neither is
/// something an operator should have to tune. The number that matters is that
/// it is not a gigabyte.
pub const MAX_HANDSHAKE_FRAME: usize = 32 * 1024;

/// How long a wire waits for a buffer before it gives up on the connection.
///
/// Long enough that a burst is absorbed as latency, which is what the bound is
/// for. Short enough that a node whose slab is genuinely exhausted tells its
/// clients so rather than holding them open forever.
const BUFFER_WAIT: Duration = Duration::from_secs(5);

/// How often it retries while waiting.
const BUFFER_RETRY: Duration = Duration::from_millis(1);

/// A socket and the buffers it is currently borrowing.
#[derive(Debug)]
pub struct Wire<S> {
    io: S,
    slab: Arc<BufferSlab>,
    /// Bytes read and not yet consumed. Absent when there are none.
    read: Option<PooledBuf>,
    /// How far into `read` the frames already handed out reached.
    ///
    /// A cursor rather than a `drain`, for the reason `FrameRelay` in
    /// `pgprox-proto` has one: draining the front of the buffer memmoves
    /// everything behind it, once per frame, and a read that pulled in a
    /// pipelined `Parse`/`Bind`/`Execute`/`Sync` pays that four times over the
    /// same tail. A profile of the running proxy put 19% of its time in
    /// `__memmove_avx_unaligned_erms`.
    ///
    /// The buffer is compacted only when the cursor reaches the end, which is
    /// the common case and costs nothing: the whole thing is dropped and the
    /// slab gets it back.
    read_at: usize,
    /// Bytes queued and not yet written. Absent when there are none.
    write: Option<PooledBuf>,
    /// What was queued while the slab had nothing to lend.
    ///
    /// Empty in every ordinary run. It exists because `queue` is synchronous
    /// and its callers have already decided to send something: dropping an
    /// `ErrorResponse` because memory was tight would leave a client with a
    /// closed socket and no reason for it.
    ///
    /// Boxed, and absent rather than empty, because a `Vec` is three words
    /// inline and this one holds nothing on any path a connection normally
    /// takes. Three words in each of the wires a session holds is a hundred
    /// bytes it pays forever for a case it reaches when the slab is exhausted.
    /// The allocation this costs happens only on that path, where the process
    /// is already out of buffers and one more is not the problem.
    ///
    /// `clippy::box_collection` says a `Vec` is already on the heap, which is
    /// true of its contents and not of its header: three words sit in this
    /// struct whether or not anything was ever queued, and this struct is
    /// instantiated per wire per connection. The extra indirection is the
    /// point rather than an oversight.
    #[allow(clippy::box_collection)]
    spare: Option<Box<Vec<u8>>>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    /// Wraps a stream, borrowing buffers from `slab` as it needs them.
    pub fn new(io: S, slab: Arc<BufferSlab>) -> Self {
        Self {
            io,
            slab,
            read: None,
            read_at: 0,
            write: None,
            spare: None,
        }
    }

    /// The stream, and any bytes already read past the current stage.
    ///
    /// Returned together on purpose. A TLS upgrade takes the stream, and the
    /// leftover bytes belong with it: dropping them is the hazard this module
    /// opens by describing.
    ///
    /// The borrowed buffer goes back to the slab here rather than travelling
    /// with the bytes, because the upgraded stream gets a new wire and would
    /// otherwise hold two.
    pub fn into_parts(self) -> (S, Vec<u8>) {
        let at = self.read_at;
        let leftover = self
            .read
            .map(|buf| buf.as_slice()[at..].to_vec())
            .unwrap_or_default();
        (self.io, leftover)
    }

    /// Whether anything has been read and not yet consumed.
    #[must_use]
    pub fn is_buffered(&self) -> bool {
        self.read
            .as_ref()
            .is_some_and(|buf| buf.as_slice().len() > self.read_at)
    }

    /// The bytes read and not yet consumed.
    fn buffered(&self) -> &[u8] {
        self.read
            .as_ref()
            .map_or(&[][..], |buf| &buf.as_slice()[self.read_at..])
    }

    /// Gives a buffer back once it holds nothing.
    ///
    /// Called at both ends of the relay loop, which is what makes an idle
    /// connection cost a socket rather than 32 KiB.
    fn reclaim(&mut self) {
        if self
            .read
            .as_ref()
            .is_some_and(|buf| buf.as_slice().len() <= self.read_at)
        {
            self.read = None;
            self.read_at = 0;
        }
        if self.write.as_ref().is_some_and(|buf| buf.is_empty()) {
            self.write = None;
        }
    }

    /// Borrows from the slab, waiting rather than allocating when it is empty.
    async fn borrow(slab: &Arc<BufferSlab>) -> Result<PooledBuf, ShellError> {
        let deadline = tokio::time::Instant::now() + BUFFER_WAIT;
        loop {
            if let Some(buf) = slab.try_borrow() {
                return Ok(buf);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ShellError::Refused(ClientError::Internal(
                    "no buffer available",
                )));
            }
            tokio::time::sleep(BUFFER_RETRY).await;
        }
    }

    /// Reads until at least one more byte arrives.
    async fn fill(&mut self) -> Result<(), ShellError> {
        // Mid-frame: a buffer is already held, so read straight into it. No
        // stack chunk, and one syscall per 16 KiB rather than per 4.
        if self.read.is_some() {
            return self.fill_held().await;
        }

        // Cold: this connection is waiting for its client to say something,
        // and it holds nothing while it waits. The first read lands in a small
        // stack chunk, and a slab buffer is borrowed only once bytes exist.
        //
        // Small on purpose. This array is alive across the await, so it is
        // part of the session future, which is part of the per-connection cost
        // of an idle connection: at 4 KiB it was 4,096 of the 11,640 bytes a
        // session cost, or 400 MB at a hundred thousand connections. A
        // statement in the reference workload is under a hundred bytes, and
        // anything longer simply continues into the borrowed buffer above.
        let mut chunk = [0_u8; FIRST_READ];
        let read = self.io.read(&mut chunk).await?;
        if read == 0 {
            return Err(ShellError::Disconnected);
        }

        let mut buf = Self::borrow(&self.slab).await?;
        buf.extend_from_slice(&chunk[..read]);
        self.read = Some(buf);
        Ok(())
    }

    /// Reads into the buffer this wire already holds.
    async fn fill_held(&mut self) -> Result<(), ShellError> {
        // The one place the consumed prefix is thrown away, and the reason the
        // cursor is affordable. Reaching here means a frame arrived split, so
        // there is a read to pay for anyway; doing it once per read rather
        // than once per frame is the whole difference. Without it the buffer
        // would keep growing past what the slab lent.
        self.compact();

        let Some(buf) = self.read.as_mut() else {
            return Err(ShellError::Disconnected);
        };

        // Read straight into uninitialised spare capacity, with no unsafe here
        // and no memset before it.
        //
        // `read_buf` passes the spare capacity to the reader as a `ReadBuf`,
        // which is tokio's way of saying "write here and tell me how much", and
        // the `unsafe` that makes that sound lives in tokio where it is already
        // verified. Nothing in this crate takes on an obligation.
        //
        // `read_buf` fills the spare capacity that is there and asks for no
        // more, so the size of a held read is decided by the line below and not
        // by whatever the slab happened to leave over. `reserve(HELD_READ)`
        // makes it the size it has always been, and cannot make the buffer
        // larger than the `resize` it replaced did, since both ask for the same
        // `len + HELD_READ`.
        //
        // Until `M30.4` this grew the buffer with `resize(.., 0)` and trimmed
        // after, and the comment saying why named a rule that does not exist:
        // that reading into uninitialised capacity needs unsafe, and that this
        // workspace forbids it. `M27` made the second half false, the first half
        // was never true, and `AsyncReadExt` was imported at the top of this
        // file throughout. It cost a 16 KiB memset on every read after a
        // connection's first.
        let vec = buf.as_mut_vec();
        vec.reserve(HELD_READ);

        // The claim above, in a form a test can fail on. Its absence is silent:
        // the frame still assembles and the buffer still stays small, and the
        // only symptom is a read that asks the kernel for sixty-four bytes.
        // `M31.1`.
        debug_assert!(
            vec.capacity() - vec.len() >= HELD_READ,
            "a held read has room for {} bytes, not {HELD_READ}",
            vec.capacity() - vec.len()
        );

        match self.io.read_buf(vec).await {
            // Nothing was appended, so there is nothing to trim back.
            Ok(0) => {
                self.read = None;
                Err(ShellError::Disconnected)
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Reads one untagged message body into `body`.
    ///
    /// # Errors
    ///
    /// Fails on disconnect, on a socket error, or on a length the codec
    /// refuses.
    pub async fn read_untagged(
        &mut self,
        body: &mut Vec<u8>,
        max_frame: usize,
    ) -> Result<(), ShellError> {
        loop {
            match decode_untagged(self.buffered(), max_frame)? {
                Decoded::Frame(frame, consumed) => {
                    body.clear();
                    body.extend_from_slice(frame.body());
                    self.consume(consumed);
                    return Ok(());
                }
                Decoded::Incomplete { .. } => self.fill().await?,
            }
        }
    }

    /// Reads one tagged message header, consuming the five bytes it occupies.
    ///
    /// The other half of [`Wire::read_tagged`], split out so a caller that does
    /// not need the body never holds one. See [`Wire::take_body`].
    ///
    /// # Safe to race on its own; the body read after it is not
    ///
    /// This call by itself is cancellation-safe: its only await is inside the
    /// buffer fill it shares with every other read here, which never consumes
    /// a byte before a complete header is available, so a future dropped here
    /// picks up exactly where it left off next time. That is what lets the
    /// client's read loop call it as a `select!` arm the drain and shed
    /// branches can win.
    ///
    /// What is *not* safe is racing anything on the read that has to follow
    /// it. The moment this returns `Ok`, the header is consumed and the wire
    /// is inside a message with a body still to come; a future dropped before
    /// that body is read leaves no way to say so. The client's read loop
    /// keeps that read (`read_client_body`, over [`Wire::read_body_into`]) on
    /// a plain unraced `.await` right after, outside the `select!`, which is
    /// what makes the pair as a whole safe there. Corrected `M90`, cycle 6: an
    /// earlier version of this doc said the header read itself was unsafe and
    /// pointed at the server-to-client pump as the one place that could use
    /// it, when the client's read loop had already been calling it from
    /// inside a `select!` since `M16.12` — correctly, but in a way this doc
    /// then contradicted.
    ///
    /// # Errors
    ///
    /// As [`Wire::read_tagged`].
    pub async fn read_header(&mut self, max_frame: usize) -> Result<FrameHeader, ShellError> {
        loop {
            if let Some(header) = decode_header(self.buffered(), max_frame)? {
                self.consume(1 + LEN_PREFIX);
                return Ok(header);
            }
            self.fill().await?;
        }
    }

    /// Reads exactly `n` body bytes into `body`, replacing what was there.
    ///
    /// The buffering half of the pair, for a message something has to read.
    /// See [`Wire::read_header`] for the safety split between the two: that
    /// call is fine to race, this one is not — call it on a plain unraced
    /// `.await` once the header it belongs to has already been read.
    ///
    /// # Errors
    ///
    /// Fails on disconnect or on a socket error.
    pub async fn read_body_into(&mut self, body: &mut Vec<u8>, n: usize) -> Result<(), ShellError> {
        body.clear();
        self.append_body(body, n).await
    }

    /// Reads `n` more body bytes onto the end of `body`.
    ///
    /// For a caller that read a prefix, found it was not enough, and wants the
    /// rest without starting again. A statement name longer than the prefix
    /// `inspect_policy` allots is rare and legal, and refusing a client over one
    /// would be a worse answer than a second read.
    ///
    /// # Errors
    ///
    /// Fails on disconnect or on a socket error.
    pub async fn append_body(&mut self, body: &mut Vec<u8>, n: usize) -> Result<(), ShellError> {
        let target = body.len() + n;
        while body.len() < target {
            let want = target - body.len();
            let chunk = self.take_body(want).await?;
            body.extend_from_slice(chunk);
        }
        self.reclaim();
        Ok(())
    }

    /// Hands out up to `n` body bytes that have already arrived.
    ///
    /// Returns what is there rather than waiting for all of it, which is what
    /// makes it a stream: the caller forwards each piece and comes back for
    /// the next, so neither side ever holds the whole message. Never returns an
    /// empty slice without having read: it fills first when nothing is
    /// buffered, so a caller counting down cannot spin.
    ///
    /// The slice borrows the wire's own read buffer, so it is valid until the
    /// next call. Nothing is copied.
    ///
    /// # Errors
    ///
    /// Fails on disconnect or on a socket error.
    pub async fn take_body(&mut self, n: usize) -> Result<&[u8], ShellError> {
        // Before the slice is taken, not after: `reclaim` can drop the buffer
        // the slice would point into.
        self.reclaim();
        if !self.is_buffered() {
            self.fill().await?;
        }

        let at = self.read_at;
        let take = n.min(self.buffered().len());
        self.read_at = at + take;
        Ok(self
            .read
            .as_ref()
            .map_or(&[][..], |buf| &buf.as_slice()[at..at + take]))
    }

    /// Drops the bytes a frame used, and the buffer with them if it is now
    /// empty.
    fn consume(&mut self, consumed: usize) {
        self.read_at += consumed;
        self.reclaim();
    }

    /// Moves the unconsumed tail to the front and forgets the cursor.
    ///
    /// The memmove this type exists to avoid, done deliberately and rarely.
    fn compact(&mut self) {
        if self.read_at == 0 {
            return;
        }
        if let Some(buf) = self.read.as_mut() {
            buf.as_mut_vec().drain(..self.read_at);
        }
        self.read_at = 0;
    }

    /// Reads one tagged message body into `body`, returning its tag.
    ///
    /// # Errors
    ///
    /// As [`Wire::read_untagged`].
    pub async fn read_tagged(
        &mut self,
        body: &mut Vec<u8>,
        max_frame: usize,
    ) -> Result<Tag, ShellError> {
        loop {
            match decode(self.buffered(), max_frame)? {
                Decoded::Frame(frame, consumed) => {
                    let tag = frame.tag();
                    body.clear();
                    body.extend_from_slice(frame.body());
                    self.consume(consumed);
                    return Ok(tag);
                }
                Decoded::Incomplete { .. } => self.fill().await?,
            }
        }
    }

    /// Builds a message into the write buffer.
    ///
    /// Synchronous, so it cannot wait for the slab. An exhausted slab here
    /// allocates one buffer rather than dropping a message the caller has
    /// already decided to send: losing an `ErrorResponse` because memory was
    /// tight would leave a client with a closed socket and no reason.
    pub fn queue(&mut self, build: impl FnOnce(&mut Vec<u8>)) {
        // Once anything has overflowed, everything after it does too, or the
        // messages would go out in the wrong order.
        if let Some(spare) = self.spare.as_mut() {
            build(spare);
            return;
        }

        match self.write.take().or_else(|| self.slab.try_borrow()) {
            Some(mut buf) => {
                build(buf.as_mut_vec());
                self.write = Some(buf);
            }
            None => build(self.spare.get_or_insert_with(Box::default)),
        }
    }

    /// The stream underneath, for the two checks that need the socket itself.
    ///
    /// Deliberately narrow: everything else goes through the framing above, and
    /// a caller reaching past it would be reading bytes the buffers are meant
    /// to own. `Upstreamed::unfit` polls it for readability and nothing else.
    pub fn io_mut(&mut self) -> &mut S {
        &mut self.io
    }

    /// Sends everything queued.
    ///
    /// # Errors
    ///
    /// Fails when the socket does.
    pub async fn flush(&mut self) -> Result<(), ShellError> {
        let queued = self.write.as_ref().is_some_and(|buf| !buf.is_empty());
        if !queued && self.spare.is_none() {
            self.write = None;
            return Ok(());
        }

        // The borrowed buffer first, then the overflow, because that is the
        // order they were built in.
        if let Some(buf) = self.write.as_ref() {
            self.io.write_all(buf.as_slice()).await?;
        }
        if let Some(spare) = self.spare.take() {
            self.io.write_all(&spare).await?;
        }
        self.io.flush().await?;

        // Emptied and handed straight back: a connection between transactions
        // holds nothing.
        self.write = None;
        Ok(())
    }

    /// Tells the client why it is being refused, then reports it.
    ///
    /// # Errors
    ///
    /// Always: the returned error is the refusal. A socket failure while
    /// sending it is swallowed, because a client that has already gone is not
    /// a more interesting fact than the refusal itself.
    pub async fn refuse(&mut self, error: ClientError) -> ShellError {
        self.queue(|out| encode::error_response(out, &error));
        let _ = self.flush().await;
        ShellError::Refused(error)
    }
}

/// Drives the startup phase to the point a credential is asked for.
///
/// Called again on the upgraded stream after [`Handoff::Upgrade`], with the
/// same [`Handshake`], which is what makes "TLS was accepted" survive the
/// change of stream type.
///
/// # Errors
///
/// Fails on disconnect, on a malformed startup packet, or on any refusal, in
/// which case the client has already been told.
pub async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    wire: &mut Wire<S>,
    handshake: &mut Handshake,
) -> Result<Handoff, ShellError> {
    let mut body = Vec::new();
    loop {
        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME).await?;
        let message = startup::decode(&body)?;
        let reply = handshake.on_startup(&message);

        if let Some(negotiation) = &reply.negotiate {
            // Every `_pq_.` parameter the client sent, by name. `M20.3`: this
            // was an empty slice, so a client asking for an extension was told
            // nothing about it, and silence is how the protocol says yes.
            let unsupported: Vec<&str> =
                negotiation.unsupported.iter().map(String::as_str).collect();
            wire.queue(|out| {
                encode::negotiate_protocol_version(out, negotiation.minor, &unsupported);
            });
        }

        match reply.action {
            Action::AcceptTls => {
                wire.queue(|out| out.push(b'S'));
                wire.flush().await?;
                return Ok(Handoff::Upgrade);
            }
            // Both refusals are the same byte. A client that asked for TLS and
            // got N decides for itself whether to continue in the clear; one
            // that asked for GSSAPI tries SSLRequest next.
            Action::RefuseTls | Action::RefuseGss => {
                wire.queue(|out| out.push(b'N'));
                wire.flush().await?;
            }
            Action::Ask(credential) => {
                wire.queue(|out| match credential {
                    Credential::Jwt => encode::authentication_cleartext_password(out),
                    Credential::Scram => encode::authentication_sasl(out, &[SCRAM_SHA_256]),
                });
                wire.flush().await?;
                return Ok(Handoff::Ask(credential));
            }
            Action::Cancel(conn) => {
                wire.flush().await?;
                return Ok(Handoff::Cancel(conn));
            }
            Action::Fail(error) => return Err(wire.refuse(error).await),
        }
    }
}

/// Reads the client's password and resolves it.
///
/// # Errors
///
/// Fails on disconnect or on any refusal, in which case the client has been
/// told. The message it gets is the same for a bad token and for a sidecar
/// that is down; the distinction survives in the returned error.
pub async fn authenticate_token<S, R>(
    wire: &mut Wire<S>,
    auth: &mut TokenAuth,
    resolver: &R,
    now: SystemTime,
) -> Result<Grant, ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    R: CredentialResolver + ?Sized,
{
    let mut body = Vec::new();
    let tag = wire.read_tagged(&mut body, MAX_HANDSHAKE_FRAME).await?;
    if tag != Tag::PASSWORD {
        return Err(wire
            .refuse(ClientError::ProtocolViolation(
                "expected a password message",
            ))
            .await);
    }

    let request = match auth.on_password(&body) {
        Progress::Resolve(request) => request,
        Progress::Fail(error) => return Err(wire.refuse(error).await),
        // Not reachable: on_password never authenticates on its own. Refused
        // rather than unwrapped, because this crate forbids panicking on a
        // path a client can reach.
        Progress::Ready(_) => {
            return Err(wire
                .refuse(ClientError::ProtocolViolation("authenticated too early"))
                .await);
        }
    };

    let answer = resolver.resolve(*request).await;
    match auth.on_resolved(answer, now) {
        Progress::Ready(grant) => Ok(*grant),
        Progress::Fail(error) => Err(wire.refuse(error).await),
        Progress::Resolve(_) => Err(wire
            .refuse(ClientError::ProtocolViolation("resolution did not settle"))
            .await),
    }
}

/// Runs the SCRAM exchange for a static user.
///
/// # Errors
///
/// Fails on disconnect or on any refusal, in which case the client has been
/// told, with the same message a bad token gets.
pub async fn authenticate_scram<S, C>(
    wire: &mut Wire<S>,
    scram: &mut ScramAuth<C>,
) -> Result<(), ShellError>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: StaticCredentials,
{
    let mut body = Vec::new();
    let mut first = true;

    loop {
        let tag = wire.read_tagged(&mut body, MAX_HANDSHAKE_FRAME).await?;
        if tag != Tag::PASSWORD {
            return Err(wire
                .refuse(ClientError::ProtocolViolation("expected a SASL message"))
                .await);
        }

        let progress = if std::mem::take(&mut first) {
            scram.on_initial(&body)
        } else {
            scram.on_response(&body)
        };

        match progress {
            SaslProgress::Continue(payload) => {
                wire.queue(|out| encode::authentication_sasl_continue(out, &payload));
                wire.flush().await?;
            }
            SaslProgress::Final(payload) => {
                wire.queue(|out| encode::authentication_sasl_final(out, &payload));
                wire.flush().await?;
                return Ok(());
            }
            SaslProgress::Fail(error) => return Err(wire.refuse(error).await),
        }
    }
}

/// Tells an authenticated client it is in.
///
/// `parameters` are the ones harvested from an upstream probe connection, so a
/// driver reading `server_version` or `client_encoding` gets the real server's
/// answer rather than something this proxy invented.
///
/// # Errors
///
/// Fails when the socket does.
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
    wire: &mut Wire<S>,
    conn: ConnId,
    parameters: &[(String, String)],
) -> Result<(), ShellError> {
    wire.queue(|out| {
        encode::authentication_ok(out);
        for (name, value) in parameters {
            encode::parameter_status(out, name, value);
        }
        encode::backend_key_data(out, conn);
        // Idle, and no upstream connection has been opened. That is the point:
        // a client that connects and sits there costs a socket and no database
        // connection at all.
        encode::ready_for_query(out, TxStatus::Idle);
    });
    wire.flush().await
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
mod tests {

    /// A slab for a test wire.
    ///
    /// Sized for one connection's worth of borrowing, which is what a test
    /// has. The bound is what makes an exhausted slab reachable in a test at
    /// all, so it is small on purpose.
    fn test_slab() -> std::sync::Arc<pgprox_core::buf::BufferSlab> {
        pgprox_core::buf::BufferSlab::new(pgprox_core::buf::DEFAULT_BUFFER_SIZE, 8)
    }
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::time::Duration;

    use pgprox_core::auth::{Backend, ClaimSet, FakeCredentialResolver, Grant, PoolHints, TlsMode};
    use pgprox_core::ids::{NodeId, ServerId, TenantId};
    use pgprox_core::secret::SecretString;
    use pgprox_proto::backend::{AuthRequest, BackendMessage};
    use tokio::io::{AsyncWriteExt, DuplexStream, duplex};

    use crate::auth::{ScramChallenge, ScramConfig};
    use crate::state::{HandshakeConfig, TlsPosture};

    /// A client's end of a duplex pair.
    struct Client(DuplexStream);

    impl Client {
        async fn send(&mut self, bytes: &[u8]) {
            self.0.write_all(bytes).await.unwrap();
        }

        async fn read_bytes(&mut self, count: usize) -> Vec<u8> {
            let mut buf = vec![0; count];
            self.0.read_exact(&mut buf).await.unwrap();
            buf
        }

        /// Reads one tagged message and decodes it.
        async fn expect(&mut self) -> (Tag, Vec<u8>) {
            let header = self.read_bytes(5).await;
            let len = u32::from_be_bytes(header[1..5].try_into().unwrap()) as usize;
            let body = self.read_bytes(len - 4).await;
            (Tag(header[0]), body)
        }

        async fn expect_auth(&mut self) -> AuthRequest {
            let (tag, body) = self.expect().await;
            let frame = pgprox_proto::frame::Frame::new(tag, &body);
            match pgprox_proto::backend::decode(&frame).unwrap() {
                BackendMessage::Authentication(request) => request,
                other => panic!("expected an authentication message, got {other:?}"),
            }
        }
    }

    // --- buffer reclaim -----------------------------------------------------

    #[tokio::test]
    async fn a_wire_that_has_read_everything_holds_no_buffer() {
        // The property the whole slab exists for: at 100k connections, an idle
        // one has to cost a socket rather than 32 KiB.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        assert_eq!(slab.outstanding(), 0, "a new wire borrowed something");

        peer.send(&untagged(b"hello")).await;
        let mut body = Vec::new();
        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap();
        assert_eq!(body, b"hello");

        assert!(!wire.is_buffered());
        assert_eq!(
            slab.outstanding(),
            0,
            "the read buffer was held after the frame was consumed"
        );
    }

    #[tokio::test]
    async fn a_header_and_its_body_are_read_as_a_pair() {
        // `M16.12`. `read_header` and `read_body_into` are the pair the pump
        // uses, and a mutation run found seven survivors between them because
        // nothing in this crate called either one. Six were in
        // `read_body_into`, including a mutant that replaced its whole body
        // with `Ok(())`.
        //
        // The body content is asserted, not just its length, because that is
        // what catches a header consuming four bytes instead of five: the body
        // would still be the right size and would start one byte early.
        // `M10.7` found exactly that mutant in `FrameRelay` and this logic is
        // its second copy.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut frame = vec![Tag::COMMAND_COMPLETE.get()];
        frame.extend_from_slice(&14_u32.to_be_bytes());
        frame.extend_from_slice(b"SELECT 42\0");
        peer.send(&frame).await;

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(header.tag, Tag::COMMAND_COMPLETE);
        assert_eq!(header.body_len, 10);

        let mut body = Vec::new();
        wire.read_body_into(&mut body, header.body_len)
            .await
            .unwrap();
        assert_eq!(body, b"SELECT 42\0", "the body started at the wrong offset");
        assert!(!wire.is_buffered(), "bytes were left behind the frame");
    }

    #[tokio::test]
    async fn a_body_split_across_reads_is_reassembled() {
        // The property the pump depends on and the one that made six mutants
        // survive: `read_body_into` loops until it has all of `n`, so a body
        // that arrives in pieces is still one body. A version that read once
        // and returned would pass a test that sent everything at once.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut header_bytes = vec![Tag::DATA_ROW.get()];
        header_bytes.extend_from_slice(&(4_u32 + 300).to_be_bytes());
        peer.send(&header_bytes).await;

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(header.body_len, 300);

        // Three writes, so the reader has to come back for more twice.
        let body_bytes: Vec<u8> = (0..300).map(|i| u8::try_from(i % 251).unwrap()).collect();
        let writer = tokio::spawn(async move {
            for piece in body_bytes.chunks(100) {
                peer.send(piece).await;
            }
            peer
        });

        let mut body = Vec::new();
        wire.read_body_into(&mut body, header.body_len)
            .await
            .unwrap();
        drop(writer.await.unwrap());

        assert_eq!(body.len(), 300, "a split body came back short");
        let expected: Vec<u8> = (0..300).map(|i| u8::try_from(i % 251).unwrap()).collect();
        assert_eq!(body, expected, "a split body came back scrambled");
    }

    #[tokio::test]
    async fn a_prefix_can_be_topped_up_without_starting_again() {
        // `M16.6`'s fallback. A statement name longer than the prefix
        // `inspect_policy` allots is rare and legal, so the relay loop reads
        // the rest rather than refusing the client. Appending, not replacing:
        // the prefix already read is the front of the same message.
        //
        // Reached only from `bin/`, which is not mutation tested, so it is
        // tested here where the function lives.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut frame = vec![Tag::BIND.get()];
        frame.extend_from_slice(&(4_u32 + 60).to_be_bytes());
        frame.extend_from_slice(&[b'p'; 60]);
        peer.send(&frame).await;

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(header.body_len, 60);

        let mut body = Vec::new();
        wire.read_body_into(&mut body, 20).await.unwrap();
        assert_eq!(body.len(), 20, "the prefix read the wrong amount");

        wire.append_body(&mut body, header.body_len - 20)
            .await
            .unwrap();
        assert_eq!(
            body.len(),
            60,
            "the top-up replaced the prefix or fell short"
        );
        assert!(body.iter().all(|byte| *byte == b'p'));
        assert!(!wire.is_buffered(), "bytes were left behind the frame");
    }

    #[tokio::test]
    async fn a_split_body_does_not_eat_the_frame_behind_it() {
        // The hazard this crate's own header calls the one that has already
        // bitten twice: a read pulls in bytes past the stage you are in.
        //
        // `read_body_into` asks for `n - body.len()`, and `take_body` hands
        // back whatever is buffered up to that. Ask for too much and a body
        // that finished mid-buffer swallows the start of the next message. A
        // mutation run made the point: `n - body.len()` became
        // `n + body.len()` and every test here stayed green.
        //
        // Two earlier versions of this test stayed green, and that is the part
        // worth keeping. The first sent the body in two writes and assumed the
        // reader would see them separately; both landed before the read began.
        // The second used a duplex large enough to hold everything, so the
        // first `take_body` satisfied the whole body at once.
        //
        // The reason either could pass is the same, and it is the thing to
        // understand before touching this: on the first pass `body.len()` is
        // zero, so `n - 0` and `n + 0` are the same number. The two
        // expressions can only disagree on a *second* pass, which means the
        // first read has to come up short and the second has to have more
        // available than is still wanted.
        //
        // Hence 120: the duplex holds exactly the first piece and no more, so
        // the writer blocks and the reader is forced to come back for a second
        // pass with a full pipe behind it. With `n + body.len()` this takes
        // 206 bytes for a 200-byte body and eats the next frame's header.
        let slab = test_slab();
        let (server, client) = duplex(120);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut first = vec![Tag::COMMAND_COMPLETE.get()];
        first.extend_from_slice(&(4_u32 + 200).to_be_bytes());
        peer.send(&first).await;

        let header = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(header.body_len, 200);

        // The rest of the body and a whole second frame behind it, from a task
        // that cannot finish until the reader makes room.
        let writer = tokio::spawn(async move {
            peer.send(&[b'a'; 120]).await;
            let mut rest = vec![b'a'; 80];
            rest.push(Tag::READY_FOR_QUERY.get());
            rest.extend_from_slice(&5_u32.to_be_bytes());
            rest.push(b'I');
            peer.send(&rest).await;
            peer
        });

        let mut body = Vec::new();
        wire.read_body_into(&mut body, header.body_len)
            .await
            .unwrap();
        assert_eq!(body.len(), 200, "the body took bytes that were not its own");
        assert!(body.iter().all(|byte| *byte == b'a'));

        // And the frame behind it is intact, which is the half that says the
        // bytes were left rather than merely counted.
        let next = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(next.tag, Tag::READY_FOR_QUERY);
        let mut status = Vec::new();
        wire.read_body_into(&mut status, next.body_len)
            .await
            .unwrap();
        assert_eq!(status, b"I");
        drop(writer.await.unwrap());
    }

    #[tokio::test]
    async fn reading_a_body_replaces_what_was_there_and_an_empty_one_is_legal() {
        // The buffer is reused across frames by every caller, so a read that
        // appended instead of replacing would grow it without bound and hand
        // the second frame the first one's bytes as well.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut two = vec![Tag::COMMAND_COMPLETE.get()];
        two.extend_from_slice(&12_u32.to_be_bytes());
        two.extend_from_slice(b"ROLLBACK");
        // Then a Sync, which carries nothing at all.
        two.push(Tag::SYNC.get());
        two.extend_from_slice(&4_u32.to_be_bytes());
        peer.send(&two).await;

        let mut body = vec![b'l', b'e', b'f', b't', b'o', b'v', b'e', b'r'];
        let first = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        wire.read_body_into(&mut body, first.body_len)
            .await
            .unwrap();
        assert_eq!(body, b"ROLLBACK", "the previous contents survived the read");

        let second = wire
            .read_header(pgprox_proto::frame::DEFAULT_MAX_FRAME)
            .await
            .unwrap();
        assert_eq!(second.tag, Tag::SYNC);
        assert_eq!(second.body_len, 0);
        wire.read_body_into(&mut body, second.body_len)
            .await
            .unwrap();
        assert!(body.is_empty(), "a body-less message left bytes behind");
    }

    #[tokio::test]
    async fn an_oversized_handshake_message_is_refused_before_it_is_read() {
        // The finding. A startup packet is the first thing a client sends and
        // it was read against DEFAULT_MAX_FRAME, a gigabyte, because that is
        // the cap for a DataRow carrying a bytea. Nothing about a handshake
        // resembles one, and this runs before the client has proved anything.
        //
        // The refusal comes from the five-byte length prefix, so the bytes
        // behind it are never read and never held. That is the property: a
        // client cannot make the proxy allocate by announcing a large number.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        // A length prefix and nothing else. The body is never sent, and if the
        // cap did not fire this would wait for it forever.
        let declared = u32::try_from(MAX_HANDSHAKE_FRAME + 1).unwrap();
        peer.send(&declared.to_be_bytes()).await;

        let mut body = Vec::new();
        let error = wire
            .read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                ShellError::Frame(pgprox_proto::frame::DecodeError::LengthTooLarge { .. })
            ),
            "an oversized startup packet was accepted: {error:?}"
        );
        assert!(body.is_empty());

        // And a handshake message of an ordinary size is still read, so the cap
        // refuses the attack rather than the protocol.
        let (server, client) = duplex(64 * 1024);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);
        let ordinary = vec![b'x'; 4096];
        peer.send(&untagged(&ordinary)).await;
        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap();
        assert_eq!(body.len(), ordinary.len());
    }

    #[tokio::test]
    async fn a_wire_mid_frame_keeps_its_buffer() {
        // The other half of the same rule. Returning a buffer with bytes still
        // in it would lose them, which is the hazard this module's header
        // describes.
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        let mut two = untagged(b"first");
        two.extend_from_slice(&untagged(b"second"));
        peer.send(&two).await;

        let mut body = Vec::new();
        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap();
        assert_eq!(body, b"first");
        assert!(wire.is_buffered(), "the second message was dropped");
        assert_eq!(slab.outstanding(), 1, "the buffer was returned too early");

        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap();
        assert_eq!(body, b"second");
        assert_eq!(slab.outstanding(), 0);
    }

    #[tokio::test]
    async fn a_flushed_wire_holds_no_write_buffer() {
        let slab = test_slab();
        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        wire.queue(encode::authentication_ok);
        assert_eq!(slab.outstanding(), 1, "queueing borrowed nothing");
        wire.flush().await.unwrap();
        assert_eq!(
            slab.outstanding(),
            0,
            "the write buffer was held after the flush"
        );

        assert_eq!(peer.expect_auth().await, AuthRequest::Ok);
    }

    #[tokio::test(start_paused = true)]
    async fn an_exhausted_slab_makes_a_read_wait_and_then_says_so() {
        // Backpressure, not allocation: a burst becomes latency rather than a
        // memory spike. And a slab that never frees up refuses the connection
        // rather than holding it open forever.
        let slab = BufferSlab::new(64, 1);
        let held = slab.try_borrow().unwrap();

        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);
        peer.send(&untagged(b"hello")).await;

        let mut body = Vec::new();
        let error = wire
            .read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap_err();
        assert!(
            matches!(error, ShellError::Refused(ClientError::Internal(_))),
            "{error}"
        );
        drop(held);
    }

    #[tokio::test]
    async fn a_buffer_freed_while_a_read_waits_is_picked_up() {
        // ADR 0008's claim, and the half of the retry loop no test ran. The
        // test above holds the slab empty for good, so the loop only ever ends
        // by giving up; both ways of breaking the deadline reach that same
        // ending and neither was visible. This is the other ending: a burst
        // becomes latency, not a refusal.
        let slab = BufferSlab::new(64, 1);
        let held = slab.try_borrow().unwrap();

        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);
        peer.send(&untagged(b"hello")).await;

        // The read is polled first and gets as far as an empty slab, so the
        // buffer comes back while it is waiting rather than before it asks.
        let mut body = Vec::new();
        let (result, ()) =
            tokio::join!(wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME), async {
                drop(held);
            });

        result.unwrap();
        assert_eq!(body, b"hello");
    }

    #[tokio::test]
    async fn a_mid_frame_read_grows_the_buffer_by_one_read_and_no_more() {
        // `fill_held` resizes to make room, reads, and trims back to what
        // arrived. Both arithmetic mistakes leave the frame decodable, which is
        // why nothing noticed: one over-trims and one over-allocates, and the
        // second is the buffer growing past what the slab lent, which is the
        // thing this type exists to stop.
        let (mut wire, mut peer) = pair();
        let mut buf = Wire::<DuplexStream>::borrow(&wire.slab).await.unwrap();
        buf.extend_from_slice(b"first");
        wire.read = Some(buf);

        peer.send(b"next").await;
        wire.fill_held().await.unwrap();

        assert_eq!(wire.buffered(), b"firstnext");
        let capacity = wire.read.as_mut().unwrap().as_mut_vec().capacity();
        assert!(
            capacity <= 2 * HELD_READ,
            "one read grew the buffer to {capacity} bytes"
        );
    }

    #[tokio::test]
    async fn a_held_read_makes_room_for_a_whole_read_before_it_reads() {
        // `reserve(HELD_READ)` is what keeps a held read the size it always
        // was. `read_buf` fills whatever spare capacity is there and asks for
        // no more, so without the reserve a buffer holding a partial frame
        // reads only what the slab happened to leave over, and a large result
        // row would arrive in a hundred syscalls instead of one.
        //
        // Nothing else here would notice: the frame still assembles, the test
        // above still sees the buffer stay small, and the only symptom is the
        // syscall count. `M30.4`.
        let (mut wire, mut peer) = pair();
        let mut buf = Wire::<DuplexStream>::borrow(&wire.slab).await.unwrap();
        let prefix = vec![b'p'; 1024];
        buf.extend_from_slice(&prefix);
        wire.read = Some(buf);

        peer.send(b"next").await;
        wire.fill_held().await.unwrap();

        let capacity = wire.read.as_mut().unwrap().as_mut_vec().capacity();
        assert!(
            capacity >= prefix.len() + HELD_READ,
            "the read had room for {} bytes past the {} already held, not {HELD_READ}",
            capacity - prefix.len(),
            prefix.len()
        );
    }

    #[tokio::test]
    async fn messages_queued_while_the_slab_is_empty_keep_their_order() {
        // Once anything has overflowed, everything after it must too. A second
        // message that found a buffer while the first was in the overflow
        // would reach the client first, and a client reading an error before
        // the answer it belongs to is worse than either message alone.
        let slab = BufferSlab::new(64, 1);
        let held = slab.try_borrow().unwrap();

        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        wire.queue(encode::authentication_ok);
        // Freed between the two, so the second would find a buffer if the
        // overflow did not hold everything after it.
        drop(held);
        wire.queue(|out| encode::ready_for_query(out, TxStatus::Idle));
        wire.flush().await.unwrap();

        assert_eq!(peer.expect_auth().await, AuthRequest::Ok);
        assert_eq!(peer.expect().await.0, Tag::READY_FOR_QUERY);
    }

    #[tokio::test]
    async fn an_exhausted_slab_still_sends_the_message_a_caller_queued() {
        // A refusal that never reaches the client leaves a driver with a
        // closed socket and no reason, which is worse than the memory.
        let slab = BufferSlab::new(64, 1);
        let held = slab.try_borrow().unwrap();

        let (server, client) = duplex(4096);
        let mut wire = Wire::new(server, Arc::clone(&slab));
        let mut peer = Client(client);

        wire.queue(|out| encode::error_response(out, &ClientError::Draining));
        wire.flush().await.unwrap();

        let (tag, body) = peer.expect().await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        // The message text is `ClientError::Draining`'s; what matters here is
        // that the frame arrived at all.
        assert!(
            String::from_utf8_lossy(&body).contains("57P01"),
            "{}",
            String::from_utf8_lossy(&body)
        );
        drop(held);
    }

    #[tokio::test]
    async fn the_cursor_and_the_buffer_agree_about_what_is_left() {
        // Four private methods that every read goes through and that no test
        // reads back: the cursor advances, the consumed prefix is dropped, an
        // exhausted buffer reports nothing, and the buffer goes back to the
        // slab exactly when it is empty. They are driven here directly rather
        // than through a socket because the interesting states are the ones
        // `reclaim` exists to make unreachable from outside.
        let (mut wire, _client) = pair();
        let mut buf = Wire::<DuplexStream>::borrow(&wire.slab).await.unwrap();
        buf.extend_from_slice(b"gonestay");
        wire.read = Some(buf);

        // The cursor is what `consume` moves, and it moves by the frame.
        wire.consume(4);
        assert_eq!(wire.read_at, 4);
        assert_eq!(wire.buffered(), b"stay");
        assert!(wire.is_buffered());

        // Four bytes still unread, so the buffer is not the slab's yet.
        assert!(
            wire.read.is_some(),
            "a buffer with unread bytes went back to the slab"
        );

        // The memmove this type exists to avoid, done deliberately: the tail
        // moves to the front and the cursor starts again.
        wire.compact();
        assert_eq!(wire.read_at, 0, "the consumed prefix was not dropped");
        assert_eq!(wire.buffered(), b"stay");
        assert_eq!(wire.read.as_ref().unwrap().as_slice(), b"stay");

        // Everything consumed, with the buffer still held. `consume` would
        // reclaim it on the way past, so the cursor is moved directly: this is
        // the state the two comparisons disagree about, and it is the one
        // `reclaim` exists to make unreachable.
        wire.read_at = 4;
        assert!(!wire.is_buffered(), "an exhausted buffer reported bytes");
        assert!(wire.read.is_some());

        wire.reclaim();
        assert!(
            wire.read.is_none(),
            "an empty buffer was not returned to the slab"
        );
        assert_eq!(wire.read_at, 0);
    }

    #[test]
    fn the_read_sizes_are_the_sizes_they_say() {
        // Both are per-connection costs stated in the comments above them, and
        // both are written as products, so an operator multiplied by a byte
        // count is one edit away from being an operator added to one. 512 is
        // the array that lives in every idle session's future; 16 KiB is what
        // a mid-frame read asks for once a buffer is already held.
        assert_eq!(FIRST_READ, 512);
        assert_eq!(HELD_READ, 16_384);
    }

    fn pair() -> (Wire<DuplexStream>, Client) {
        let (server, client) = duplex(4096);
        (Wire::new(server, test_slab()), Client(client))
    }

    /// An untagged message: a length prefix and a body.
    fn untagged(body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 4) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    fn ssl_request() -> Vec<u8> {
        untagged(&startup::SSL_REQUEST_CODE.to_be_bytes())
    }

    fn startup_packet(user: &str, database: &str) -> Vec<u8> {
        startup_packet_with(user, database, &[])
    }

    /// The same, plus whatever extra parameters the client sent.
    fn startup_packet_with(user: &str, database: &str, extra: &[(&str, &str)]) -> Vec<u8> {
        let mut body = 196_608_i32.to_be_bytes().to_vec();
        for (name, value) in [("user", user), ("database", database)]
            .iter()
            .chain(extra.iter())
        {
            body.extend_from_slice(name.as_bytes());
            body.push(0);
            body.extend_from_slice(value.as_bytes());
            body.push(0);
        }
        body.push(0);
        untagged(&body)
    }

    fn password(text: &str) -> Vec<u8> {
        let mut out = vec![Tag::PASSWORD.get()];
        out.extend_from_slice(&((text.len() + 5) as u32).to_be_bytes());
        out.extend_from_slice(text.as_bytes());
        out.push(0);
        out
    }

    fn sasl_initial(mechanism: &str, payload: &str) -> Vec<u8> {
        let mut body = mechanism.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(&(payload.len() as i32).to_be_bytes());
        body.extend_from_slice(payload.as_bytes());

        let mut out = vec![Tag::PASSWORD.get()];
        out.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn sasl_response(payload: &str) -> Vec<u8> {
        let mut out = vec![Tag::PASSWORD.get()];
        out.extend_from_slice(&((payload.len() + 4) as u32).to_be_bytes());
        out.extend_from_slice(payload.as_bytes());
        out
    }

    fn grant() -> Grant {
        Grant {
            tenant: TenantId::new("acme"),
            primary: Backend {
                server: ServerId::new("db-1", 5432),
                database: "acme".into(),
                user: "acme_app".into(),
                password: SecretString::new("hunter2"),
                tls: TlsMode::Verified,
            },
            replicas: Vec::new(),
            pool: PoolHints::default(),
            ttl: Duration::from_secs(60),
            claims: ClaimSet::default(),
        }
    }

    fn optional_tls() -> Handshake {
        Handshake::new(HandshakeConfig {
            tls: TlsPosture::Optional,
            ..HandshakeConfig::default()
        })
    }

    #[tokio::test]
    async fn a_client_asking_for_tls_is_told_to_upgrade() {
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig::default());

        client.send(&ssl_request()).await;
        let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();

        assert_eq!(handoff, Handoff::Upgrade);
        assert_eq!(client.read_bytes(1).await, b"S");
        assert!(handshake.is_encrypted());
    }

    #[tokio::test]
    async fn a_refused_ssl_request_is_answered_and_the_session_continues() {
        // N is not the end of the conversation: libpq falls back to plaintext
        // and sends its startup packet next.
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig {
            tls: TlsPosture::Disabled,
            ..HandshakeConfig::default()
        });

        client.send(&ssl_request()).await;
        client.send(&startup_packet("acme_app", "acme")).await;

        let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();
        assert_eq!(client.read_bytes(1).await, b"N");
        assert_eq!(handoff, Handoff::Ask(Credential::Jwt));
    }

    #[tokio::test]
    async fn a_protocol_extension_is_declined_on_the_wire_by_name() {
        // `M20.3`. The whole path, because the two halves that were wrong were
        // in different crates: the handshake decided from the version alone,
        // and the one caller of the encoder passed an empty list. Either one
        // left alone still tells a client its extension was accepted.
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();

        client
            .send(&startup_packet_with(
                "acme_app",
                "acme",
                &[("_pq_.some_extension", "1")],
            ))
            .await;
        let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();

        // The message itself: newest minor, then a count, then the names.
        let (tag, body) = client.expect().await;
        assert_eq!(tag, Tag::NEGOTIATE_PROTOCOL_VERSION);
        assert_eq!(i32::from_be_bytes(body[0..4].try_into().unwrap()), 0);
        assert_eq!(
            i32::from_be_bytes(body[4..8].try_into().unwrap()),
            1,
            "the count did not say one option was unrecognised: {body:?}"
        );
        assert_eq!(
            String::from_utf8_lossy(&body[8..body.len() - 1]),
            "_pq_.some_extension",
            "the client was not told which option it did not get"
        );

        // And the connection carries on. The extension was declined, not the
        // client.
        assert_eq!(handoff, Handoff::Ask(Credential::Jwt));
        assert_eq!(client.expect_auth().await, AuthRequest::CleartextPassword);
    }

    #[tokio::test]
    async fn a_token_client_is_asked_for_a_cleartext_password() {
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();

        client.send(&startup_packet("acme_app", "acme")).await;
        let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();

        assert_eq!(handoff, Handoff::Ask(Credential::Jwt));
        assert_eq!(client.expect_auth().await, AuthRequest::CleartextPassword);
    }

    #[tokio::test]
    async fn a_static_user_is_offered_sasl() {
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig {
            tls: TlsPosture::Optional,
            static_users: vec!["pgprox_admin".to_owned()],
        });

        client.send(&startup_packet("pgprox_admin", "pgprox")).await;
        let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();

        assert_eq!(handoff, Handoff::Ask(Credential::Scram));
        assert_eq!(client.expect_auth().await, AuthRequest::Sasl);
    }

    #[tokio::test]
    async fn a_client_that_skipped_tls_where_it_is_required_is_told_why() {
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig::default());

        client.send(&startup_packet("acme_app", "acme")).await;
        let err = negotiate(&mut wire, &mut handshake).await.unwrap_err();

        assert!(matches!(err, ShellError::Refused(ClientError::TlsRequired)));
        let (tag, body) = client.expect().await;
        assert_eq!(tag, Tag::ERROR_RESPONSE);
        assert!(
            String::from_utf8_lossy(&body).contains("28000"),
            "the error carried no SQLSTATE a driver could act on"
        );
    }

    #[tokio::test]
    async fn a_cancel_request_ends_the_conversation() {
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig::default());
        let conn = ConnId::new(NodeId::new(2), 77);

        let mut body = startup::CANCEL_REQUEST_CODE.to_be_bytes().to_vec();
        let (process_id, secret) = pgprox_proto::backend::key_from_conn_id(conn);
        body.extend_from_slice(&process_id.to_be_bytes());
        body.extend_from_slice(&secret.to_be_bytes());

        client.send(&untagged(&body)).await;
        assert_eq!(
            negotiate(&mut wire, &mut handshake).await.unwrap(),
            Handoff::Cancel(conn)
        );
    }

    #[tokio::test]
    async fn a_whole_token_session_runs_over_a_duplex_pair() {
        // The acceptance criterion: no port opened, no TLS, no sidecar
        // process, and every message on the wire is a real one.
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();
        let resolver = Arc::new(FakeCredentialResolver::new().with_grant("good.token", grant()));

        client.send(&startup_packet("acme_app", "acme")).await;
        assert_eq!(
            negotiate(&mut wire, &mut handshake).await.unwrap(),
            Handoff::Ask(Credential::Jwt)
        );
        assert_eq!(client.expect_auth().await, AuthRequest::CleartextPassword);

        let mut auth = TokenAuth::new(
            handshake.startup().unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        client.send(&password("good.token")).await;
        let granted =
            authenticate_token(&mut wire, &mut auth, resolver.as_ref(), SystemTime::now())
                .await
                .unwrap();
        assert_eq!(granted.tenant, TenantId::new("acme"));

        let parameters = vec![("server_version".to_owned(), "17.2".to_owned())];
        accept(&mut wire, ConnId::new(NodeId::new(1), 5), &parameters)
            .await
            .unwrap();

        assert_eq!(client.expect_auth().await, AuthRequest::Ok);
        assert_eq!(client.expect().await.0, Tag::PARAMETER_STATUS);
        assert_eq!(client.expect().await.0, Tag::BACKEND_KEY_DATA);
        let (tag, body) = client.expect().await;
        assert_eq!(tag, Tag::READY_FOR_QUERY);
        assert_eq!(body, b"I", "the client was told a transaction was open");
    }

    #[tokio::test]
    async fn a_rejected_token_reaches_the_client_as_an_error_response() {
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();
        let resolver = Arc::new(FakeCredentialResolver::new());

        client.send(&startup_packet("acme_app", "acme")).await;
        negotiate(&mut wire, &mut handshake).await.unwrap();
        client.expect_auth().await;

        let mut auth = TokenAuth::new(
            handshake.startup().unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        client.send(&password("no.such.token")).await;
        let err = authenticate_token(&mut wire, &mut auth, resolver.as_ref(), SystemTime::now())
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            ShellError::Refused(ClientError::AuthRefused(_))
        ));
        assert_eq!(client.expect().await.0, Tag::ERROR_RESPONSE);
    }

    #[tokio::test]
    async fn a_client_that_sends_the_wrong_message_instead_of_a_password_is_refused() {
        let (mut wire, mut client) = pair();
        let resolver = Arc::new(FakeCredentialResolver::new());
        let mut auth = TokenAuth::new(
            &crate::state::StartupInfo {
                user: "acme_app".to_owned(),
                database: "acme".to_owned(),
                settings: Vec::new(),
            },
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );

        // A Query, where a PasswordMessage was asked for.
        client.send(&[b'Q', 0, 0, 0, 5, 0]).await;
        let err = authenticate_token(&mut wire, &mut auth, resolver.as_ref(), SystemTime::now())
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            ShellError::Refused(ClientError::ProtocolViolation(_))
        ));
    }

    /// One user, one proof, no crypto: the arithmetic lives outside this crate.
    #[derive(Debug)]
    struct OneUser;

    impl StaticCredentials for OneUser {
        fn challenge(&self, user: &str) -> Option<ScramChallenge> {
            (user == "pgprox_admin").then(|| ScramChallenge {
                salt: "QSXCR+Q6sek8bf92".to_owned(),
                iterations: 4096,
            })
        }

        fn verify(&self, _user: &str, _auth_message: &str, proof: &str) -> Option<String> {
            (proof == "cHJvb2Y=").then(|| "c2lnbmF0dXJl".to_owned())
        }
    }

    #[tokio::test]
    async fn a_whole_scram_session_runs_over_a_duplex_pair() {
        let (mut wire, mut client) = pair();
        let mut scram = ScramAuth::new(
            OneUser,
            ScramConfig {
                mock_salt: "bW9jaw==".to_owned(),
                mock_iterations: 4096,
            },
            "pgprox_admin",
            "SERVERNONCE",
        );

        client
            .send(&sasl_initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"))
            .await;
        client
            .send(&sasl_response("c=biws,r=CLIENTNONCESERVERNONCE,p=cHJvb2Y="))
            .await;

        authenticate_scram(&mut wire, &mut scram).await.unwrap();

        assert_eq!(client.expect_auth().await, AuthRequest::SaslContinue);
        assert_eq!(client.expect_auth().await, AuthRequest::SaslFinal);
    }

    #[tokio::test]
    async fn a_scram_client_with_the_wrong_proof_gets_an_error_response() {
        let (mut wire, mut client) = pair();
        let mut scram = ScramAuth::new(
            OneUser,
            ScramConfig {
                mock_salt: "bW9jaw==".to_owned(),
                mock_iterations: 4096,
            },
            "pgprox_admin",
            "SERVERNONCE",
        );

        client
            .send(&sasl_initial(SCRAM_SHA_256, "n,,n=,r=CLIENTNONCE"))
            .await;
        client
            .send(&sasl_response("c=biws,r=CLIENTNONCESERVERNONCE,p=d3Jvbmc="))
            .await;

        let err = authenticate_scram(&mut wire, &mut scram).await.unwrap_err();
        assert!(matches!(
            err,
            ShellError::Refused(ClientError::AuthRefused(_))
        ));
    }

    #[tokio::test]
    async fn a_client_that_disconnects_mid_handshake_is_reported_rather_than_hung() {
        let (mut wire, client) = pair();
        let mut handshake = optional_tls();
        drop(client);

        assert!(matches!(
            negotiate(&mut wire, &mut handshake).await.unwrap_err(),
            ShellError::Disconnected
        ));
    }

    #[tokio::test]
    async fn a_frame_split_across_reads_is_reassembled() {
        // The kernel decides where a read ends, not the protocol. A shell that
        // assumed one read is one message would fail against any client whose
        // packet happened to split, which is most of them under load.
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();
        let packet = startup_packet("acme_app", "acme");
        let (head, tail) = packet.split_at(6);

        client.send(head).await;
        let task = tokio::spawn(async move {
            let handoff = negotiate(&mut wire, &mut handshake).await.unwrap();
            (handoff, wire)
        });
        client.send(tail).await;

        let (handoff, _wire) = task.await.unwrap();
        assert_eq!(handoff, Handoff::Ask(Credential::Jwt));
    }

    #[tokio::test]
    async fn many_frames_from_one_read_come_back_in_order() {
        // The cursor's correctness case. Consuming a frame no longer moves the
        // bytes behind it, so a read that pulled in four of them hands them
        // out from four different offsets into the same buffer. An off-by-one
        // here reads a frame from the middle of its neighbour, which is the
        // failure mode a `drain` cannot have.
        let (mut wire, mut client) = pair();

        let mut pipelined = Vec::new();
        for sql in ["SELECT 1", "SELECT 22", "SELECT 333", "SELECT 4444"] {
            pgprox_proto::encode_frontend::query(&mut pipelined, sql);
        }
        client.send(&pipelined).await;

        let mut body = Vec::new();
        for sql in ["SELECT 1", "SELECT 22", "SELECT 333", "SELECT 4444"] {
            let tag = wire
                .read_tagged(&mut body, MAX_HANDSHAKE_FRAME)
                .await
                .unwrap();
            assert_eq!(tag, Tag::QUERY);
            assert_eq!(
                String::from_utf8_lossy(&body).trim_end_matches('\0'),
                sql,
                "a frame came back from the wrong offset"
            );
        }

        assert!(
            !wire.is_buffered(),
            "the buffer still holds bytes after four frames were consumed"
        );
    }

    #[tokio::test]
    async fn a_frame_split_behind_consumed_ones_is_still_reassembled() {
        // The cursor's other half. Three frames arrive, two are consumed, and
        // the third is cut in the middle: the next read has to land behind the
        // partial frame rather than behind the consumed ones. Without the
        // compaction in `fill_held` it would, and the frame would decode as
        // whatever the arithmetic said.
        let (mut wire, mut client) = pair();

        let mut first = Vec::new();
        pgprox_proto::encode_frontend::query(&mut first, "SELECT 1");
        pgprox_proto::encode_frontend::query(&mut first, "SELECT 22");
        let mut third = Vec::new();
        pgprox_proto::encode_frontend::query(&mut third, "SELECT 333");
        let (head, tail) = third.split_at(7);
        first.extend_from_slice(head);
        client.send(&first).await;

        let mut body = Vec::new();
        for sql in ["SELECT 1", "SELECT 22"] {
            wire.read_tagged(&mut body, MAX_HANDSHAKE_FRAME)
                .await
                .unwrap();
            assert_eq!(String::from_utf8_lossy(&body).trim_end_matches('\0'), sql);
        }

        let task = tokio::spawn(async move {
            let mut body = Vec::new();
            wire.read_tagged(&mut body, MAX_HANDSHAKE_FRAME)
                .await
                .unwrap();
            body
        });
        client.send(tail).await;

        let body = task.await.unwrap();
        assert_eq!(
            String::from_utf8_lossy(&body).trim_end_matches('\0'),
            "SELECT 333",
            "the split frame was reassembled from the wrong offset"
        );
    }

    #[tokio::test]
    async fn bytes_read_past_a_stage_survive_into_the_next_one() {
        // The hazard this crate's AGENTS.md names. The client pipelines its
        // password behind its startup packet, so one read pulls in both; a
        // shell that dropped the remainder would wait for a message it had
        // already been sent.
        let (mut wire, mut client) = pair();
        let mut handshake = optional_tls();
        let resolver = Arc::new(FakeCredentialResolver::new().with_grant("good.token", grant()));

        let mut pipelined = startup_packet("acme_app", "acme");
        pipelined.extend_from_slice(&password("good.token"));
        client.send(&pipelined).await;

        negotiate(&mut wire, &mut handshake).await.unwrap();
        assert!(
            wire.is_buffered(),
            "the pipelined password was not read, so this proves nothing"
        );

        let mut auth = TokenAuth::new(
            handshake.startup().unwrap(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
        );
        let granted =
            authenticate_token(&mut wire, &mut auth, resolver.as_ref(), SystemTime::now())
                .await
                .unwrap();
        assert_eq!(granted.tenant, TenantId::new("acme"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancelled_read_loses_no_bytes() {
        // Dropping the future mid-frame is normal: it happens whenever a
        // session loses a select! race against a drain. The bytes that arrived
        // belong to the connection, so the next call continues from them.
        //
        // The clock is paused, so the timeout below fires without the suite
        // spending twenty real milliseconds on it.
        let (mut wire, mut client) = pair();
        let packet = startup_packet("acme_app", "acme");
        let (head, tail) = packet.split_at(6);
        client.send(head).await;

        let mut body = Vec::new();
        let cancelled = tokio::time::timeout(
            Duration::from_millis(20),
            wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME),
        )
        .await;
        assert!(
            cancelled.is_err(),
            "the read completed, so nothing was cancelled"
        );

        client.send(tail).await;
        wire.read_untagged(&mut body, MAX_HANDSHAKE_FRAME)
            .await
            .unwrap();
        assert!(
            !body.is_empty(),
            "the bytes read before the cancellation were lost"
        );
    }

    #[tokio::test]
    async fn a_leftover_buffer_travels_with_the_stream_on_upgrade() {
        // into_parts returns both together on purpose: a TLS upgrade takes the
        // stream, and anything read past the SSLRequest belongs with it.
        let (mut wire, mut client) = pair();
        let mut handshake = Handshake::new(HandshakeConfig::default());

        client.send(&ssl_request()).await;
        negotiate(&mut wire, &mut handshake).await.unwrap();

        let (_stream, leftover) = wire.into_parts();
        assert!(
            leftover.is_empty(),
            "bytes were read past the SSLRequest, and a client would not have sent any"
        );
    }
}
