//! IMAP flag add (`SELECT` + `STORE +FLAGS`), wrapping a private
//! orchestrator that selects the mailbox then stores the requested
//! flags.

use core::mem;

use alloc::vec::Vec;

use io_imap::{
    context::ImapContext,
    rfc3501::{
        select::{ImapMailboxSelect, ImapMailboxSelectError, ImapMailboxSelectResult},
        store::{ImapMessageStore, ImapMessageStoreError, ImapMessageStoreResult},
    },
    types::{
        flag::{Flag as ImapFlag, StoreType},
        mailbox::Mailbox as ImapMailbox,
        sequence::SequenceSet,
    },
};
use log::trace;
use thiserror::Error;

/// Errors produced while orchestrating SELECT + STORE for IMAP flag add.
#[derive(Debug, Error)]
pub enum ImapFlagAddError {
    #[error(transparent)]
    Select(#[from] ImapMailboxSelectError),
    #[error(transparent)]
    Store(#[from] ImapMessageStoreError),
    #[error("IMAP flag add was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`ImapFlagAdd::resume`].
#[derive(Debug)]
pub enum ImapFlagAddResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapFlagAddError),
}

enum State {
    Selecting {
        select: ImapMailboxSelect,
        sequence_set: SequenceSet,
        flags: Vec<ImapFlag<'static>>,
        uid: bool,
    },
    Storing(ImapMessageStore),
    Done,
}

/// I/O-free coroutine wrapping `SELECT <mailbox>` followed by `STORE
/// <sequence-set> +FLAGS <flags>`.
pub struct ImapFlagAdd {
    state: State,
}

impl ImapFlagAdd {
    pub fn new(
        context: ImapContext,
        mailbox: ImapMailbox<'static>,
        sequence_set: SequenceSet,
        flags: Vec<ImapFlag<'static>>,
        uid: bool,
    ) -> Self {
        trace!("prepare IMAP flag add");
        Self {
            state: State::Selecting {
                select: ImapMailboxSelect::new(context, mailbox),
                sequence_set,
                flags,
                uid,
            },
        }
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> ImapFlagAddResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Selecting {
                    mut select,
                    sequence_set,
                    flags,
                    uid,
                } => match select.resume(arg.take()) {
                    ImapMailboxSelectResult::WantsRead => {
                        self.state = State::Selecting {
                            select,
                            sequence_set,
                            flags,
                            uid,
                        };
                        return ImapFlagAddResult::WantsRead;
                    }
                    ImapMailboxSelectResult::WantsWrite(bytes) => {
                        self.state = State::Selecting {
                            select,
                            sequence_set,
                            flags,
                            uid,
                        };
                        return ImapFlagAddResult::WantsWrite(bytes);
                    }
                    ImapMailboxSelectResult::Err { err, .. } => {
                        return ImapFlagAddResult::Err(err.into());
                    }
                    ImapMailboxSelectResult::Ok { context, .. } => {
                        let store = ImapMessageStore::new(
                            context,
                            sequence_set,
                            StoreType::Add,
                            flags,
                            uid,
                        );
                        self.state = State::Storing(store);
                    }
                },
                State::Storing(mut store) => match store.resume(arg.take()) {
                    ImapMessageStoreResult::WantsRead => {
                        self.state = State::Storing(store);
                        return ImapFlagAddResult::WantsRead;
                    }
                    ImapMessageStoreResult::WantsWrite(bytes) => {
                        self.state = State::Storing(store);
                        return ImapFlagAddResult::WantsWrite(bytes);
                    }
                    ImapMessageStoreResult::Err { err, .. } => {
                        return ImapFlagAddResult::Err(err.into());
                    }
                    ImapMessageStoreResult::Ok { .. } => return ImapFlagAddResult::Ok,
                },
                State::Done => return ImapFlagAddResult::Err(ImapFlagAddError::AlreadyDone),
            }
        }
    }
}
