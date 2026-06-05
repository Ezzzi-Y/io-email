//! Maildir message-get coroutine.
//!
//! Wraps [`io_maildir::entry::get::MaildirEntryGet`]: locates the
//! message inside the resolved mailbox via the embedded
//! [`MaildirEntryLocate`](io_maildir::entry::locate::MaildirEntryLocate)
//! (cur → new → tmp probing) and reads its raw RFC 5322 bytes.

use alloc::vec::Vec;

use io_maildir::{
    coroutine::*,
    entry::get::{
        MaildirEntryGet as InnerMaildirMessageGet,
        MaildirEntryGetError as InnerMaildirMessageGetError,
    },
    maildir::types::Maildir,
    store::MaildirStore,
};
use log::trace;
use thiserror::Error;

use crate::maildir::convert::{InvalidMailboxName, mailbox_path};

/// Errors produced by [`MaildirMessageGet`].
#[derive(Debug, Error)]
pub enum MaildirMessageGetError {
    #[error(transparent)]
    Get(#[from] InnerMaildirMessageGetError),
    #[error(transparent)]
    InvalidMailbox(#[from] InvalidMailboxName),
}

/// I/O-free coroutine reading a single Maildir message as raw bytes.
pub struct MaildirMessageGet {
    inner: InnerMaildirMessageGet,
}

impl MaildirMessageGet {
    pub fn new(
        store: &MaildirStore,
        mailbox: &str,
        id: &str,
    ) -> Result<Self, MaildirMessageGetError> {
        trace!("prepare Maildir message get");
        let path = mailbox_path(mailbox)?;
        let maildir = Maildir::from_path(store.resolve(&path));
        Ok(Self {
            inner: InnerMaildirMessageGet::new(maildir, id),
        })
    }
}

impl MaildirCoroutine for MaildirMessageGet {
    type Yield = MaildirYield;
    type Return = Result<Vec<u8>, MaildirMessageGetError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(arg) {
            MaildirCoroutineState::Yielded(y) => MaildirCoroutineState::Yielded(y),
            MaildirCoroutineState::Complete(Ok(entry)) => {
                MaildirCoroutineState::Complete(Ok(entry.into()))
            }
            MaildirCoroutineState::Complete(Err(err)) => {
                MaildirCoroutineState::Complete(Err(err.into()))
            }
        }
    }
}
