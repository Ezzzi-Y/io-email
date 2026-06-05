//! Maildir message-delete coroutine.
//!
//! Maildir has no atomic "remove this message" primitive; the
//! conventional approach is to mark the message with the `T`
//! (Trashed) info-section letter and let a periodic expunge clean it
//! up. This coroutine wraps [`io_maildir::flag::add::MaildirFlagsAdd`]
//! with the [`MaildirFlag::Trashed`] flag so a `delete_message` call on
//! the shared API stays portable.
//!
//! [`MaildirFlag::Trashed`]: io_maildir::flag::types::MaildirFlag::Trashed

use core::iter::once;

use io_maildir::{
    coroutine::*,
    flag::{
        add::{MaildirFlagsAdd as InnerAdd, MaildirFlagsAddError as InnerErr},
        types::MaildirFlag,
    },
    maildir::types::Maildir,
    store::MaildirStore,
};
use log::trace;
use thiserror::Error;

use crate::maildir::convert::{InvalidMailboxName, mailbox_path};

/// Errors produced by [`MaildirMessageDelete`].
#[derive(Debug, Error)]
pub enum MaildirMessageDeleteError {
    #[error(transparent)]
    Trash(#[from] InnerErr),
    #[error(transparent)]
    InvalidMailbox(#[from] InvalidMailboxName),
}

/// I/O-free coroutine flagging a Maildir message as Trashed.
pub struct MaildirMessageDelete {
    inner: InnerAdd,
}

impl MaildirMessageDelete {
    pub fn new(
        store: &MaildirStore,
        mailbox: &str,
        id: &str,
    ) -> Result<Self, MaildirMessageDeleteError> {
        trace!("prepare Maildir message delete (Trashed flag)");
        let path = mailbox_path(mailbox)?;
        let maildir = Maildir::from_path(store.resolve(&path));
        let trashed = once(MaildirFlag::Trashed).collect();
        Ok(Self {
            inner: InnerAdd::new(maildir, id, trashed),
        })
    }
}

impl MaildirCoroutine for MaildirMessageDelete {
    type Yield = MaildirYield;
    type Return = Result<(), MaildirMessageDeleteError>;

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
