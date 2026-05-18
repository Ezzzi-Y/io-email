//! IMAP message get (`SELECT` + `FETCH BODY.PEEK[]`), wrapping a private
//! orchestrator. Returns raw RFC 5322 bytes.

use core::{mem, num::NonZeroU32};

use alloc::vec::Vec;

use io_imap::{
    context::ImapContext,
    rfc3501::{
        fetch::{ImapMessageFetchError, ImapMessageFetchFirst, ImapMessageFetchFirstResult},
        select::{ImapMailboxSelect, ImapMailboxSelectError, ImapMailboxSelectResult},
    },
    types::{
        fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName},
        mailbox::Mailbox as ImapMailbox,
    },
};
use log::trace;
use thiserror::Error;

/// Errors produced while orchestrating SELECT + FETCH for IMAP message
/// retrieval.
#[derive(Debug, Error)]
pub enum ImapMessageGetError {
    #[error(transparent)]
    Select(#[from] ImapMailboxSelectError),
    #[error(transparent)]
    Fetch(#[from] ImapMessageFetchError),
    #[error("FETCH did not return any body for the requested message")]
    Empty,
    #[error("IMAP message get was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`ImapMessageGet::resume`].
#[derive(Debug)]
pub enum ImapMessageGetResult {
    Ok(Vec<u8>),
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapMessageGetError),
}

enum State {
    Selecting {
        select: ImapMailboxSelect,
        id: NonZeroU32,
        uid: bool,
    },
    Fetching(ImapMessageFetchFirst),
    Done,
}

/// I/O-free coroutine wrapping `SELECT <mailbox>` followed by `FETCH
/// <id> BODY.PEEK[]`.
pub struct ImapMessageGet {
    state: State,
}

impl ImapMessageGet {
    pub fn new(
        context: ImapContext,
        mailbox: ImapMailbox<'static>,
        id: NonZeroU32,
        uid: bool,
    ) -> Self {
        trace!("prepare IMAP message get");
        Self {
            state: State::Selecting {
                select: ImapMailboxSelect::new(context, mailbox),
                id,
                uid,
            },
        }
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> ImapMessageGetResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Selecting {
                    mut select,
                    id,
                    uid,
                } => match select.resume(arg.take()) {
                    ImapMailboxSelectResult::WantsRead => {
                        self.state = State::Selecting { select, id, uid };
                        return ImapMessageGetResult::WantsRead;
                    }
                    ImapMailboxSelectResult::WantsWrite(bytes) => {
                        self.state = State::Selecting { select, id, uid };
                        return ImapMessageGetResult::WantsWrite(bytes);
                    }
                    ImapMailboxSelectResult::Err { err, .. } => {
                        return ImapMessageGetResult::Err(err.into());
                    }
                    ImapMailboxSelectResult::Ok { context, .. } => {
                        let item_names = MacroOrMessageDataItemNames::MessageDataItemNames(vec![
                            MessageDataItemName::BodyExt {
                                section: None,
                                partial: None,
                                peek: true,
                            },
                        ]);
                        let fetch = ImapMessageFetchFirst::new(context, id, item_names, uid);
                        self.state = State::Fetching(fetch);
                    }
                },
                State::Fetching(mut fetch) => match fetch.resume(arg.take()) {
                    ImapMessageFetchFirstResult::WantsRead => {
                        self.state = State::Fetching(fetch);
                        return ImapMessageGetResult::WantsRead;
                    }
                    ImapMessageFetchFirstResult::WantsWrite(bytes) => {
                        self.state = State::Fetching(fetch);
                        return ImapMessageGetResult::WantsWrite(bytes);
                    }
                    ImapMessageFetchFirstResult::Err { err, .. } => {
                        return ImapMessageGetResult::Err(err.into());
                    }
                    ImapMessageFetchFirstResult::Ok { items, .. } => {
                        let raw = items.into_inner().into_iter().find_map(|item| match item {
                            MessageDataItem::BodyExt { data, .. } => {
                                data.0.map(|d| d.as_ref().to_vec())
                            }
                            _ => None,
                        });

                        let Some(raw) = raw else {
                            return ImapMessageGetResult::Err(ImapMessageGetError::Empty);
                        };

                        return ImapMessageGetResult::Ok(raw);
                    }
                },
                State::Done => {
                    return ImapMessageGetResult::Err(ImapMessageGetError::AlreadyDone);
                }
            }
        }
    }
}
