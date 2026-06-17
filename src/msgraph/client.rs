//! Std-blocking Microsoft Graph client.
//!
//! Holds an inner [`MsgraphClientStd`] (from io-msgraph) wrapping the
//! boxed stream, the OAuth 2.0 bearer credential and the target user id
//! (usually `me`). Graph is folder-based and stateless over HTTP: there
//! is no session and no account-global change token, so the diff/watch
//! shared-API methods are not implemented.
//!
//! [`MsgraphClientStd::run`] pumps io-email Microsoft Graph coroutines
//! against the inner client's stream; [`MsgraphClientStd::inner`] stays
//! reachable for protocol-specific paths (raw folder/message calls).
//!
//! [`MsgraphClientStd`]: io_msgraph::v1::client::MsgraphClientStd

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use std::io::{self, Read, Write};

use io_msgraph::{
    coroutine::*,
    v1::client::{
        MsgraphClientStd as Inner, MsgraphClientStdConnectOptions,
        MsgraphClientStdError as InnerError,
    },
};
#[cfg(any(
    feature = "rustls-ring",
    feature = "rustls-aws",
    feature = "native-tls"
))]
use pimalaya_stream::tls::Tls;
use thiserror::Error;

use crate::{
    envelope::{
        msgraph::list::{MsgraphEnvelopeList, MsgraphEnvelopeListError},
        types::Envelope,
    },
    flag::{
        msgraph::store::{MsgraphFlagStore, MsgraphFlagStoreError},
        types::{Flag, FlagOp},
    },
    mailbox::{
        msgraph::{
            create::{MsgraphMailboxCreate, MsgraphMailboxCreateError},
            delete::{MsgraphMailboxDelete, MsgraphMailboxDeleteError},
            list::{MsgraphMailboxList, MsgraphMailboxListError},
        },
        types::Mailbox,
    },
    message::msgraph::{
        copy::{MsgraphMessageCopy, MsgraphMessageCopyError},
        delete::{MsgraphMessageDelete, MsgraphMessageDeleteError},
        get::{MsgraphMessageGet, MsgraphMessageGetError},
        r#move::{MsgraphMessageMove, MsgraphMessageMoveError},
        send::{MsgraphMessageSend, MsgraphMessageSendError},
    },
};

/// Errors surfaced by [`MsgraphClientStd`] while running a coroutine.
///
/// One variant per shared-API Microsoft Graph coroutine.
#[derive(Debug, Error)]
pub enum MsgraphClientError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    MailboxList(#[from] MsgraphMailboxListError),
    #[error(transparent)]
    EnvelopeList(#[from] MsgraphEnvelopeListError),
    #[error(transparent)]
    FlagStore(#[from] MsgraphFlagStoreError),
    #[error(transparent)]
    MailboxCreate(#[from] MsgraphMailboxCreateError),
    #[error(transparent)]
    MailboxDelete(#[from] MsgraphMailboxDeleteError),
    #[error(transparent)]
    MessageGet(#[from] MsgraphMessageGetError),
    #[error(transparent)]
    MessageDelete(#[from] MsgraphMessageDeleteError),
    #[error(transparent)]
    MessageCopy(#[from] MsgraphMessageCopyError),
    #[error(transparent)]
    MessageMove(#[from] MsgraphMessageMoveError),
    #[error(transparent)]
    MessageSend(#[from] MsgraphMessageSendError),
    #[error(transparent)]
    Inner(#[from] InnerError),
}

const READ_BUFFER_SIZE: usize = 16 * 1024;

/// Light Microsoft Graph client built on top of the io-msgraph inner.
pub struct MsgraphClientStd {
    pub inner: Inner,
}

impl MsgraphClientStd {
    /// Wraps an already-connected stream with the bare OAuth 2.0 bearer
    /// access token (the client adds the `Bearer ` prefix itself) and
    /// the Graph user id (usually `me`).
    pub fn new<S: Read + Write + Send + 'static>(
        stream: S,
        token: impl ToString,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            inner: Inner::new(
                stream,
                token,
                MsgraphClientStdConnectOptions {
                    user_id: user_id.into(),
                    ..Default::default()
                },
            ),
        }
    }

    /// Pumps any standard-shape Microsoft Graph coroutine
    /// (`Yield = MsgraphYield`, `Return = Result<T, E>`) against the
    /// inner client's stream until it terminates.
    pub fn run<C, T, E>(&mut self, mut coroutine: C) -> Result<T, MsgraphClientError>
    where
        C: MsgraphCoroutine<Yield = MsgraphYield, Return = Result<T, E>>,
        MsgraphClientError: From<E>,
    {
        let mut buf = [0u8; READ_BUFFER_SIZE];
        let mut arg: Option<&[u8]> = None;

        loop {
            match coroutine.resume(arg.take()) {
                MsgraphCoroutineState::Complete(Ok(out)) => return Ok(out),
                MsgraphCoroutineState::Complete(Err(err)) => return Err(err.into()),
                MsgraphCoroutineState::Yielded(MsgraphYield::WantsRead) => {
                    let n = self.inner.stream.read(&mut buf)?;
                    arg = Some(&buf[..n]);
                }
                MsgraphCoroutineState::Yielded(MsgraphYield::WantsWrite(bytes)) => {
                    self.inner.stream.write_all(&bytes)?;
                }
            }
        }
    }

    /// Lists every Graph mail folder as a [`Mailbox`]. Graph folders
    /// carry their total/unread counts inline, so `with_counts` is
    /// ignored (counts are always present).
    pub fn list_mailboxes(
        &mut self,
        with_counts: bool,
    ) -> Result<Vec<Mailbox>, MsgraphClientError> {
        let coroutine =
            MsgraphMailboxList::new(&self.inner.auth, &self.inner.user_id, with_counts)?;
        self.run(coroutine)
    }

    /// Lists envelopes from the `mailbox` folder (id or well-known name
    /// such as `inbox`). `page` is 1-indexed; the offset is `(page - 1)
    /// * page_size` via OData `$skip`.
    pub fn list_envelopes(
        &mut self,
        mailbox: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<Vec<Envelope>, MsgraphClientError> {
        let coroutine = MsgraphEnvelopeList::new(
            &self.inner.auth,
            &self.inner.user_id,
            mailbox,
            page,
            page_size,
        )?;
        self.run(coroutine)
    }

    /// Adds, sets or removes `flags` on a message id set via one `PATCH
    /// /messages/{id}` per id. `mailbox` is unused: Graph messages are
    /// addressed globally by id.
    pub fn store_flags(
        &mut self,
        mailbox: &str,
        ids: &[&str],
        flags: &[Flag],
        op: FlagOp,
    ) -> Result<(), MsgraphClientError> {
        let coroutine = MsgraphFlagStore::new(
            &self.inner.auth,
            &self.inner.user_id,
            mailbox,
            ids,
            flags,
            op,
        )?;
        self.run(coroutine)
    }

    /// Fetches one message's raw RFC 5322 bytes via `GET
    /// /messages/{id}/$value`. `mailbox` is unused.
    pub fn get_message(&mut self, mailbox: &str, id: &str) -> Result<Vec<u8>, MsgraphClientError> {
        let coroutine = MsgraphMessageGet::new(&self.inner.auth, &self.inner.user_id, mailbox, id)?;
        self.run(coroutine)
    }

    /// Creates `name` as a new Graph mail folder under the mailbox root.
    pub fn create_mailbox(&mut self, name: &str) -> Result<(), MsgraphClientError> {
        let coroutine = MsgraphMailboxCreate::new(&self.inner.auth, &self.inner.user_id, name)?;
        self.run(coroutine)
    }

    /// Deletes the Graph mail folder `id` and everything in it.
    pub fn delete_mailbox(&mut self, id: &str) -> Result<(), MsgraphClientError> {
        let coroutine = MsgraphMailboxDelete::new(&self.inner.auth, &self.inner.user_id, id)?;
        self.run(coroutine)
    }

    /// Permanently deletes the Graph message `id`. `mailbox` is unused.
    pub fn delete_message(&mut self, mailbox: &str, id: &str) -> Result<(), MsgraphClientError> {
        let coroutine =
            MsgraphMessageDelete::new(&self.inner.auth, &self.inner.user_id, mailbox, id)?;
        self.run(coroutine)
    }

    /// Copies a message id set into `to` via one `POST
    /// /messages/{id}/copy` per id. `from` is unused.
    pub fn copy_messages(
        &mut self,
        from: &str,
        to: &str,
        ids: &[&str],
    ) -> Result<(), MsgraphClientError> {
        let coroutine =
            MsgraphMessageCopy::new(&self.inner.auth, &self.inner.user_id, from, to, ids)?;
        self.run(coroutine)
    }

    /// Moves a message id set into `to` via one `POST
    /// /messages/{id}/move` per id. `from` is unused.
    pub fn move_messages(
        &mut self,
        from: &str,
        to: &str,
        ids: &[&str],
    ) -> Result<(), MsgraphClientError> {
        let coroutine =
            MsgraphMessageMove::new(&self.inner.auth, &self.inner.user_id, from, to, ids)?;
        self.run(coroutine)
    }

    /// Sends a raw RFC 5322 message via `POST /sendMail` (MIME form);
    /// Graph saves it to Sent Items.
    pub fn send_message(&mut self, raw: Vec<u8>) -> Result<(), MsgraphClientError> {
        let coroutine = MsgraphMessageSend::new(&self.inner.auth, &self.inner.user_id, raw)?;
        self.run(coroutine)
    }
}

#[cfg(any(
    feature = "rustls-ring",
    feature = "rustls-aws",
    feature = "native-tls"
))]
impl MsgraphClientStd {
    /// Opens a TLS connection to the Microsoft Graph API and builds the
    /// inner client around it. `user_id` is the mailbox owner (usually
    /// `me`).
    pub fn connect(
        tls: &Tls,
        token: impl ToString,
        user_id: impl Into<String>,
    ) -> Result<Self, MsgraphClientError> {
        let options = MsgraphClientStdConnectOptions {
            tls: tls.clone(),
            user_id: user_id.into(),
        };
        let inner = Inner::connect(token, options)?;
        Ok(Self { inner })
    }
}
