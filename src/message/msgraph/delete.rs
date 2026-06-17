//! Microsoft Graph message-delete coroutine wrapping `DELETE
//! /messages/{id}`; a permanent delete.

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::messages::delete::MsgraphMessageDelete as InnerDelete,
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMessageDelete`].
#[derive(Debug, Error)]
pub enum MsgraphMessageDeleteError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine permanently deleting a Graph message by id.
pub struct MsgraphMessageDelete {
    inner: InnerDelete,
}

impl MsgraphMessageDelete {
    /// `mailbox` is unused; kept for shared-API symmetry.
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        _mailbox: &str,
        id: &str,
    ) -> Result<Self, MsgraphMessageDeleteError> {
        trace!("prepare Microsoft Graph message delete");
        Ok(Self {
            inner: InnerDelete::new(auth, user_id, id)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMessageDelete {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphMessageDeleteError>;

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
