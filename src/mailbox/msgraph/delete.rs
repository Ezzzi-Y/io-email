//! Microsoft Graph mailbox-delete coroutine wrapping `DELETE
//! /mailFolders/{id}`; the folder and everything in it is removed.

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::mail_folders::delete::MsgraphMailFolderDelete,
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMailboxDelete`].
#[derive(Debug, Error)]
pub enum MsgraphMailboxDeleteError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine deleting a Graph mail folder by id.
pub struct MsgraphMailboxDelete {
    inner: MsgraphMailFolderDelete,
}

impl MsgraphMailboxDelete {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        id: &str,
    ) -> Result<Self, MsgraphMailboxDeleteError> {
        trace!("prepare Microsoft Graph mailbox delete");
        Ok(Self {
            inner: MsgraphMailFolderDelete::new(auth, user_id, id)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMailboxDelete {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphMailboxDeleteError>;

    fn resume(&mut self, bytes: Option<&[u8]>) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(bytes) {
            MsgraphCoroutineState::Yielded(y) => MsgraphCoroutineState::Yielded(y),
            MsgraphCoroutineState::Complete(Ok(_)) => MsgraphCoroutineState::Complete(Ok(())),
            MsgraphCoroutineState::Complete(Err(err)) => {
                MsgraphCoroutineState::Complete(Err(err.into()))
            }
        }
    }
}
