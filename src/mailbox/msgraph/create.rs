//! Microsoft Graph mailbox-create coroutine wrapping `POST
//! /mailFolders`; the folder is created under the mailbox root.

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::mail_folders::{MsgraphMailFolder, create::MsgraphMailFolderCreate},
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMailboxCreate`].
#[derive(Debug, Error)]
pub enum MsgraphMailboxCreateError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine creating a Graph mail folder named `name`.
pub struct MsgraphMailboxCreate {
    inner: MsgraphMailFolderCreate,
}

impl MsgraphMailboxCreate {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        name: &str,
    ) -> Result<Self, MsgraphMailboxCreateError> {
        trace!("prepare Microsoft Graph mailbox create");
        let folder = MsgraphMailFolder {
            display_name: name.into(),
            ..Default::default()
        };
        Ok(Self {
            inner: MsgraphMailFolderCreate::new(auth, user_id, &folder)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMailboxCreate {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphMailboxCreateError>;

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
