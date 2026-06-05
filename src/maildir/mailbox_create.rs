//! Maildir mailbox-create coroutine.
//!
//! Wraps [`io_maildir::maildir::create::MaildirCreate`]: creates the
//! `<mailbox>/`, `<mailbox>/cur/`, `<mailbox>/new/` and `<mailbox>/tmp/`
//! directories. The on-disk name is computed from the configured
//! [`MaildirStore`] (fs or Maildir++ layout).

use io_maildir::{
    coroutine::*,
    maildir::create::{
        MaildirCreate as InnerMaildirCreate, MaildirCreateError as InnerMaildirCreateError,
    },
    store::MaildirStore,
};
use log::trace;
use thiserror::Error;

use crate::maildir::convert::{InvalidMailboxName, mailbox_path};

/// Errors produced by [`MaildirMailboxCreate`].
#[derive(Debug, Error)]
pub enum MaildirMailboxCreateError {
    #[error(transparent)]
    Create(#[from] InnerMaildirCreateError),
    #[error(transparent)]
    InvalidMailbox(#[from] InvalidMailboxName),
}

/// I/O-free coroutine creating a Maildir mailbox under the configured
/// store.
pub struct MaildirMailboxCreate {
    inner: InnerMaildirCreate,
}

impl MaildirMailboxCreate {
    pub fn new(store: &MaildirStore, name: &str) -> Result<Self, MaildirMailboxCreateError> {
        trace!("prepare Maildir mailbox create");
        let path = mailbox_path(name)?;
        Ok(Self {
            inner: InnerMaildirCreate::new(store, path),
        })
    }
}

impl MaildirCoroutine for MaildirMailboxCreate {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirMailboxCreateError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(arg) {
            MaildirCoroutineState::Yielded(y) => MaildirCoroutineState::Yielded(y),
            MaildirCoroutineState::Complete(r) => {
                MaildirCoroutineState::Complete(r.map_err(Into::into))
            }
        }
    }
}
