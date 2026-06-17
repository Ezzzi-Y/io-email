//! Microsoft Graph flag-store coroutine: one `PATCH /messages/{id}` per
//! id, translating flags to a message patch body.
//!
//! Graph models flags as scalar message fields (`isRead`, the follow-up
//! flag, `importance`) plus `categories`; flags without a Graph
//! equivalent are dropped. `mailbox` is part of the shared signature but
//! unused: Graph messages are addressed globally by id.

use alloc::{boxed::Box, string::String, vec::Vec};
use core::mem;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::messages::{MsgraphMessage, update::MsgraphMessageUpdate},
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

use crate::{
    flag::types::{Flag, FlagOp},
    msgraph::convert::flag_patch,
};

/// Errors produced by [`MsgraphFlagStore`].
#[derive(Debug, Error)]
pub enum MsgraphFlagStoreError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
    #[error("coroutine was resumed after completion")]
    ResumedAfterDone,
}

/// I/O-free coroutine applying a flag store across every id.
pub struct MsgraphFlagStore {
    state: State,
    auth: HttpAuthBearer,
    user_id: String,
    ids: Vec<String>,
    patch: MsgraphMessage,
}

impl MsgraphFlagStore {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        _mailbox: &str,
        ids: &[&str],
        flags: &[Flag],
        op: FlagOp,
    ) -> Result<Self, MsgraphFlagStoreError> {
        trace!("prepare Microsoft Graph flag store ({op:?})");
        let patch = flag_patch(flags, op);
        let ids: Vec<String> = ids.iter().map(|id| (*id).into()).collect();

        let state = if ids.is_empty() || patch == Default::default() {
            State::Noop
        } else {
            let current = Box::new(MsgraphMessageUpdate::new(auth, user_id, &ids[0], &patch)?);
            State::Updating { index: 0, current }
        };

        Ok(Self {
            state,
            auth: auth.clone(),
            user_id: user_id.into(),
            ids,
            patch,
        })
    }
}

enum State {
    Updating {
        index: usize,
        current: Box<MsgraphMessageUpdate>,
    },
    Noop,
    Done,
}

impl MsgraphCoroutine for MsgraphFlagStore {
    type Yield = MsgraphYield;
    type Return = Result<(), MsgraphFlagStoreError>;

    fn resume(
        &mut self,
        mut bytes: Option<&[u8]>,
    ) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Updating { index, mut current } => match current.resume(bytes) {
                    MsgraphCoroutineState::Yielded(y) => {
                        self.state = State::Updating { index, current };
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
                        let current = match MsgraphMessageUpdate::new(
                            &self.auth,
                            &self.user_id,
                            &self.ids[index],
                            &self.patch,
                        ) {
                            Ok(update) => Box::new(update),
                            Err(err) => return MsgraphCoroutineState::Complete(Err(err.into())),
                        };
                        self.state = State::Updating { index, current };
                        bytes = None;
                    }
                },
                State::Noop => return MsgraphCoroutineState::Complete(Ok(())),
                State::Done => {
                    return MsgraphCoroutineState::Complete(Err(
                        MsgraphFlagStoreError::ResumedAfterDone,
                    ));
                }
            }
        }
    }
}
