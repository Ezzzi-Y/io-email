//! Microsoft Graph message-copy coroutine: one `POST
//! /messages/{id}/copy` per id into the destination folder.
//!
//! The source `from` is unused (shared-API symmetry); Graph copies are
//! addressed by message id and destination folder.

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::mem;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::messages::copy::MsgraphMessageCopy as InnerCopy,
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

/// Errors produced by [`MsgraphMessageCopy`].
#[derive(Debug, Error)]
pub enum MsgraphMessageCopyError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
    #[error("coroutine was resumed after completion")]
    ResumedAfterDone,
}

/// I/O-free coroutine copying every id into `to`.
pub struct MsgraphMessageCopy {
    state: State,
    auth: HttpAuthBearer,
    user_id: String,
    ids: Vec<String>,
    to: String,
}

impl MsgraphMessageCopy {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        _from: &str,
        to: &str,
        ids: &[&str],
    ) -> Result<Self, MsgraphMessageCopyError> {
        trace!("prepare Microsoft Graph message copy");
        let ids: Vec<String> = ids.iter().map(|id| (*id).into()).collect();
        let to = to.to_string();

        let state = if ids.is_empty() {
            State::Noop
        } else {
            let current = Box::new(InnerCopy::new(auth, user_id, &ids[0], &to)?);
            State::Copying { index: 0, current }
        };

        Ok(Self {
            state,
            auth: auth.clone(),
            user_id: user_id.into(),
            ids,
            to,
        })
    }
}

enum State {
    Copying {
        index: usize,
        current: Box<InnerCopy>,
    },
    Noop,
    Done,
}

impl MsgraphCoroutine for MsgraphMessageCopy {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphMessageCopyError>;

    fn resume(
        &mut self,
        mut bytes: Option<&[u8]>,
    ) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Copying { index, mut current } => match current.resume(bytes) {
                    MsgraphCoroutineState::Yielded(y) => {
                        self.state = State::Copying { index, current };
                        return MsgraphCoroutineState::Yielded(y);
                    }
                    MsgraphCoroutineState::Complete(Err(err)) => {
                        return MsgraphCoroutineState::Complete(Err(err.into()));
                    }
                    MsgraphCoroutineState::Complete(Ok(_)) => {
                        let index = index + 1;
                        if index >= self.ids.len() {
                            return MsgraphCoroutineState::Complete(Ok(()));
                        }
                        let current = match InnerCopy::new(
                            &self.auth,
                            &self.user_id,
                            &self.ids[index],
                            &self.to,
                        ) {
                            Ok(copy) => Box::new(copy),
                            Err(err) => return MsgraphCoroutineState::Complete(Err(err.into())),
                        };
                        self.state = State::Copying { index, current };
                        bytes = None;
                    }
                },
                State::Noop => return MsgraphCoroutineState::Complete(Ok(())),
                State::Done => {
                    return MsgraphCoroutineState::Complete(Err(
                        MsgraphMessageCopyError::ResumedAfterDone,
                    ));
                }
            }
        }
    }
}
