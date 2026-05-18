//! IMAP message copy (`SELECT <src>` + `UID COPY <ids> <dst>`), wrapping
//! a private orchestrator.

use core::mem;

use alloc::vec::Vec;

use io_imap::{
    context::ImapContext,
    rfc3501::{
        copy::{
            ImapMessageCopy as InnerImapMessageCopy,
            ImapMessageCopyError as InnerImapMessageCopyError,
            ImapMessageCopyResult as InnerImapMessageCopyResult,
        },
        select::{ImapMailboxSelect, ImapMailboxSelectError, ImapMailboxSelectResult},
    },
    types::{mailbox::Mailbox as ImapMailbox, sequence::SequenceSet},
};
use log::trace;
use thiserror::Error;

/// Errors produced while orchestrating SELECT + UID COPY for IMAP
/// message copy.
#[derive(Debug, Error)]
pub enum ImapMessageCopyError {
    #[error(transparent)]
    Select(#[from] ImapMailboxSelectError),
    #[error(transparent)]
    Copy(#[from] InnerImapMessageCopyError),
    #[error("IMAP message copy was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`ImapMessageCopy::resume`].
#[derive(Debug)]
pub enum ImapMessageCopyResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapMessageCopyError),
}

enum State {
    Selecting {
        select: ImapMailboxSelect,
        sequence_set: SequenceSet,
        target: ImapMailbox<'static>,
        uid: bool,
    },
    Copying(InnerImapMessageCopy),
    Done,
}

/// I/O-free coroutine wrapping `SELECT <from>` followed by `UID COPY
/// <ids> <to>`. UIDs by default; pass `uid = false` to interpret the
/// sequence-set as message sequence numbers.
pub struct ImapMessageCopy {
    state: State,
}

impl ImapMessageCopy {
    pub fn new(
        context: ImapContext,
        from: ImapMailbox<'static>,
        to: ImapMailbox<'static>,
        sequence_set: SequenceSet,
        uid: bool,
    ) -> Self {
        trace!("prepare IMAP message copy");
        Self {
            state: State::Selecting {
                select: ImapMailboxSelect::new(context, from),
                sequence_set,
                target: to,
                uid,
            },
        }
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> ImapMessageCopyResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Selecting {
                    mut select,
                    sequence_set,
                    target,
                    uid,
                } => match select.resume(arg.take()) {
                    ImapMailboxSelectResult::WantsRead => {
                        self.state = State::Selecting {
                            select,
                            sequence_set,
                            target,
                            uid,
                        };
                        return ImapMessageCopyResult::WantsRead;
                    }
                    ImapMailboxSelectResult::WantsWrite(bytes) => {
                        self.state = State::Selecting {
                            select,
                            sequence_set,
                            target,
                            uid,
                        };
                        return ImapMessageCopyResult::WantsWrite(bytes);
                    }
                    ImapMailboxSelectResult::Err { err, .. } => {
                        return ImapMessageCopyResult::Err(err.into());
                    }
                    ImapMailboxSelectResult::Ok { context, .. } => {
                        let copy = InnerImapMessageCopy::new(context, sequence_set, target, uid);
                        self.state = State::Copying(copy);
                    }
                },
                State::Copying(mut copy) => match copy.resume(arg.take()) {
                    InnerImapMessageCopyResult::WantsRead => {
                        self.state = State::Copying(copy);
                        return ImapMessageCopyResult::WantsRead;
                    }
                    InnerImapMessageCopyResult::WantsWrite(bytes) => {
                        self.state = State::Copying(copy);
                        return ImapMessageCopyResult::WantsWrite(bytes);
                    }
                    InnerImapMessageCopyResult::Err { err, .. } => {
                        return ImapMessageCopyResult::Err(err.into());
                    }
                    InnerImapMessageCopyResult::Ok { .. } => return ImapMessageCopyResult::Ok,
                },
                State::Done => {
                    return ImapMessageCopyResult::Err(ImapMessageCopyError::AlreadyDone);
                }
            }
        }
    }
}
