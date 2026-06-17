//! Microsoft Graph envelope-list coroutine wrapping `GET
//! /mailFolders/{folder}/messages`.
//!
//! Graph returns the full message resource, so a single listing fills
//! the envelopes; `page` is 1-indexed and the offset is `(page - 1) *
//! page_size` via OData `$skip`.

use alloc::vec::Vec;

use io_http::rfc6750::bearer::HttpAuthBearer;
use io_msgraph::{
    coroutine::{MsgraphCoroutine, MsgraphCoroutineState, MsgraphYield},
    v1::rest::users::messages::list::{MsgraphMessagesList, MsgraphMessagesListParams},
    v1::send::MsgraphSendError,
};
use log::trace;
use thiserror::Error;

use crate::{
    envelope::types::Envelope,
    msgraph::convert::{ENVELOPE_SELECT, envelope_from},
};

/// Errors produced by [`MsgraphEnvelopeList`].
#[derive(Debug, Error)]
pub enum MsgraphEnvelopeListError {
    #[error(transparent)]
    Send(#[from] MsgraphSendError),
}

/// I/O-free coroutine listing envelopes from a Graph mail folder.
pub struct MsgraphEnvelopeList {
    inner: MsgraphMessagesList,
}

impl MsgraphEnvelopeList {
    pub fn new(
        auth: &HttpAuthBearer,
        user_id: &str,
        mailbox: &str,
        page: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<Self, MsgraphEnvelopeListError> {
        trace!("prepare Microsoft Graph envelope listing");
        let page = page.unwrap_or(1).max(1);
        let skip = page_size.map(|size| (page - 1) * size);

        let params = MsgraphMessagesListParams {
            top: page_size,
            skip,
            select: Some(ENVELOPE_SELECT),
            orderby: Some("receivedDateTime desc"),
            ..Default::default()
        };
        Ok(Self {
            inner: MsgraphMessagesList::new(auth, user_id, Some(mailbox), &params)?,
        })
    }
}

impl MsgraphCoroutine for MsgraphEnvelopeList {
    type Yield = MsgraphYield;
    type Return = Result<Vec<Envelope>, MsgraphEnvelopeListError>;

    fn resume(&mut self, bytes: Option<&[u8]>) -> MsgraphCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(bytes) {
            MsgraphCoroutineState::Yielded(y) => MsgraphCoroutineState::Yielded(y),
            MsgraphCoroutineState::Complete(Err(err)) => {
                MsgraphCoroutineState::Complete(Err(err.into()))
            }
            MsgraphCoroutineState::Complete(Ok(out)) => {
                let envelopes = out.response.value.into_iter().map(envelope_from).collect();
                MsgraphCoroutineState::Complete(Ok(envelopes))
            }
        }
    }
}
