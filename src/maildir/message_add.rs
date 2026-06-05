//! Maildir message-add coroutine.
//!
//! Wraps [`io_maildir::entry::store::MaildirEntryStore`]: writes the
//! raw bytes to `tmp/`, then renames into `cur/` with the info-section
//! letters derived from `flags`. The yielded id is the Maildir filename
//! minus the `:2,FLAGS` suffix.
//!
//! `MaildirEntryStore` itself probes time / pid / hostname to mint the
//! message identifier (RFC's `time.usec.hostname` convention), so this
//! coroutine relays those `Wants*` variants through.

use alloc::{string::String, vec::Vec};

use io_maildir::{
    coroutine::*,
    entry::store::{MaildirEntryStore as InnerStore, MaildirEntryStoreError as InnerErr},
    maildir::types::{Maildir, MaildirSubdir},
    store::MaildirStore,
};
use log::trace;
use thiserror::Error;

use crate::{
    flag::Flag,
    maildir::convert::{InvalidMailboxName, flags_to_maildir, mailbox_path},
};

/// Errors produced by [`MaildirMessageAdd`].
#[derive(Debug, Error)]
pub enum MaildirMessageAddError {
    #[error(transparent)]
    Store(#[from] InnerErr),
    #[error(transparent)]
    InvalidMailbox(#[from] InvalidMailboxName),
}

/// I/O-free coroutine appending a raw message to a Maildir under `cur/`.
pub struct MaildirMessageAdd {
    inner: InnerStore,
}

impl MaildirMessageAdd {
    pub fn new(
        store: &MaildirStore,
        mailbox: &str,
        flags: &[Flag],
        bytes: Vec<u8>,
    ) -> Result<Self, MaildirMessageAddError> {
        trace!("prepare Maildir message add");
        let path = mailbox_path(mailbox)?;
        let maildir = Maildir::from_path(store.resolve(&path));
        let md_flags = flags_to_maildir(flags);
        Ok(Self {
            inner: InnerStore::new(maildir, MaildirSubdir::Cur, md_flags, bytes),
        })
    }
}

impl MaildirCoroutine for MaildirMessageAdd {
    type Yield = MaildirYield;
    type Return = Result<String, MaildirMessageAddError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(arg) {
            MaildirCoroutineState::Yielded(y) => MaildirCoroutineState::Yielded(y),
            MaildirCoroutineState::Complete(Ok(ok)) => MaildirCoroutineState::Complete(Ok(ok.id)),
            MaildirCoroutineState::Complete(Err(err)) => {
                MaildirCoroutineState::Complete(Err(err.into()))
            }
        }
    }
}
