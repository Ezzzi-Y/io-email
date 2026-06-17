//! Microsoft Graph message-send coroutine wrapping `POST /sendMail`
//! (MIME form); Graph saves the message to Sent Items.

use alloc::vec::Vec;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::send_mail::MsgraphSendMailMime as InnerSend,
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMessageSend`].
#[derive(Debug, Error)]
pub enum MsgraphMessageSendError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine sending a raw RFC 5322 message through Graph.
pub struct MsgraphMessageSend {
    inner: InnerSend,
}

impl MsgraphMessageSend {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        raw: Vec<u8>,
    ) -> Result<Self, MsgraphMessageSendError> {
        trace!("prepare Microsoft Graph message send");
        Ok(Self {
            inner: InnerSend::new(auth, user_id, &raw)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphMessageSend {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphMessageSendError>;

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
