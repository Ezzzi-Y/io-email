//! IMAP message add (`APPEND <mailbox> [flags] <bytes>`), wrapping
//! [`io_imap::rfc3501::append::ImapMessageAppend`].

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use io_imap::{
    context::ImapContext,
    rfc3501::append::{
        ImapMessageAppend as InnerImapMessageAppend, ImapMessageAppendError,
        ImapMessageAppendResult,
    },
    types::{
        core::Literal, extensions::binary::LiteralOrLiteral8, flag::Flag as ImapFlag,
        mailbox::Mailbox as ImapMailbox,
    },
};
use log::trace;
use thiserror::Error;

/// Errors produced while running IMAP APPEND.
#[derive(Debug, Error)]
pub enum ImapMessageAddError {
    #[error(transparent)]
    Append(#[from] ImapMessageAppendError),
    #[error("Failed to encode the message as an IMAP literal: {0}")]
    Literal(String),
    #[error("IMAP message add was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`ImapMessageAdd::resume`].
///
/// `appenduid` is `Some((uidvalidity, uid))` when the server returned an
/// `[APPENDUID …]` response code (RFC 4315).
#[derive(Debug)]
pub enum ImapMessageAddResult {
    Ok(Option<(u32, u32)>),
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapMessageAddError),
}

/// I/O-free coroutine wrapping `APPEND <mailbox> [flags] <bytes>`.
pub struct ImapMessageAdd {
    inner: Option<InnerImapMessageAppend>,
}

impl ImapMessageAdd {
    /// `flags` are written verbatim into the APPEND command; pass an
    /// empty vec to leave the message unflagged.
    pub fn new(
        context: ImapContext,
        mailbox: ImapMailbox<'static>,
        flags: Vec<ImapFlag<'static>>,
        raw: Vec<u8>,
    ) -> Result<Self, ImapMessageAddError> {
        trace!("prepare IMAP message add");
        let literal =
            Literal::try_from(raw).map_err(|err| ImapMessageAddError::Literal(err.to_string()))?;
        let message = LiteralOrLiteral8::Literal(literal);
        let inner = InnerImapMessageAppend::new(context, mailbox, flags, None, message);
        Ok(Self { inner: Some(inner) })
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> ImapMessageAddResult {
        let Some(mut append) = self.inner.take() else {
            return ImapMessageAddResult::Err(ImapMessageAddError::AlreadyDone);
        };

        match append.resume(arg) {
            ImapMessageAppendResult::WantsRead => {
                self.inner = Some(append);
                ImapMessageAddResult::WantsRead
            }
            ImapMessageAppendResult::WantsWrite(bytes) => {
                self.inner = Some(append);
                ImapMessageAddResult::WantsWrite(bytes)
            }
            ImapMessageAppendResult::Err { err, .. } => ImapMessageAddResult::Err(err.into()),
            ImapMessageAppendResult::Ok { appenduid, .. } => ImapMessageAddResult::Ok(appenduid),
        }
    }
}
