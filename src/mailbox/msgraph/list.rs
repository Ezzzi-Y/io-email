//! Microsoft Graph list-mailboxes coroutine wrapping `GET
//! /mailFolders`.
//!
//! Graph mail folders carry their total/unread counts inline, so
//! `with_counts` is ignored: counts are always present.

use alloc::vec::Vec;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::mail_folders::list::{MsgraphMailFoldersList, MsgraphMailFoldersListParams},
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

use crate::{mailbox::types::Mailbox, msgraph::convert::mailbox_from};

/// Upper bound on mail folders fetched in one listing page; mailboxes
/// rarely exceed this, and Graph paginates beyond it.
const MAILBOXES_PAGE_SIZE: u32 = 200;

/// Errors produced by [`MsgraphMailboxList`].
#[derive(Debug, Error)]
pub enum MsgraphMailboxListError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine listing every Graph mail folder as a [`Mailbox`].
pub struct MsgraphMailboxList {
    inner: MsgraphMailFoldersList,
}

impl MsgraphMailboxList {
    /// `with_counts` is unused: Graph folders always carry their counts.
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        _with_counts: bool,
    ) -> Result<Self, MsgraphMailboxListError> {
        trace!("prepare Microsoft Graph mailbox listing");
        let params = MsgraphMailFoldersListParams {
            top: Some(MAILBOXES_PAGE_SIZE),
            ..Default::default()
        };
        Ok(Self {
            inner: MsgraphMailFoldersList::new(auth, user_id, &params)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMailboxList {
    type Yield = MsgraphYield;
    type Return = Result<Vec<Mailbox>, MsgraphMailboxListError>;

    fn resume(&mut self, bytes: Option<&[u8]>) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(bytes) {
            MsgraphCoroutineState::Yielded(y) => MsgraphCoroutineState::Yielded(y),
            MsgraphCoroutineState::Complete(Err(err)) => {
                MsgraphCoroutineState::Complete(Err(err.into()))
            }
            MsgraphCoroutineState::Complete(Ok(out)) => {
                let mailboxes = out.response.value.into_iter().map(mailbox_from).collect();
                MsgraphCoroutineState::Complete(Ok(mailboxes))
            }
        }
    }
}
