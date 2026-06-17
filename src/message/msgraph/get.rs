//! Microsoft Graph message-get coroutine wrapping `GET
//! /messages/{id}/$value`, which returns the raw RFC 5322 bytes.

use alloc::vec::Vec;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::messages::get_raw::MsgraphMessageGetRaw,
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMessageGet`].
#[derive(Debug, Error)]
pub enum MsgraphMessageGetError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine fetching the raw RFC 5322 bytes of a Graph message.
pub struct MsgraphMessageGet {
    inner: MsgraphMessageGetRaw,
}

impl MsgraphMessageGet {
    /// `mailbox` is unused; kept for shared-API symmetry.
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        _mailbox: &str,
        id: &str,
    ) -> Result<Self, MsgraphMessageGetError> {
        trace!("prepare Microsoft Graph message get");
        Ok(Self {
            inner: MsgraphMessageGetRaw::new(auth, user_id, id)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMessageGet {
    type Yield = MsgraphYield;
    type Return = Result<Vec<u8>, MsgraphMessageGetError>;

    fn resume(&mut self, bytes: Option<&[u8]>) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(bytes) {
            MsgraphCoroutineState::Yielded(y) => MsgraphCoroutineState::Yielded(y),
            MsgraphCoroutineState::Complete(Err(err)) => {
                MsgraphCoroutineState::Complete(Err(err.into()))
            }
            MsgraphCoroutineState::Complete(Ok(out)) => {
                MsgraphCoroutineState::Complete(Ok(out.response))
            }
        }
    }
}
