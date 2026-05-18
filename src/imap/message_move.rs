//! IMAP message move (`SELECT <src>` + `UID MOVE <ids> <dst>`, RFC
//! 6851), wrapping a private orchestrator.

use core::mem;

use alloc::vec::Vec;

use io_imap::{
    context::ImapContext,
    rfc3501::select::{ImapMailboxSelect, ImapMailboxSelectError, ImapMailboxSelectResult},
    rfc6851::r#move::{
        ImapMessageMove as InnerImapMessageMove, ImapMessageMoveError as InnerImapMessageMoveError,
        ImapMessageMoveResult as InnerImapMessageMoveResult,
    },
    types::{mailbox::Mailbox as ImapMailbox, sequence::SequenceSet},
};
use log::trace;
use thiserror::Error;

/// Errors produced while orchestrating SELECT + UID MOVE for IMAP
/// message move.
#[derive(Debug, Error)]
pub enum ImapMessageMoveError {
    #[error(transparent)]
    Select(#[from] ImapMailboxSelectError),
    #[error(transparent)]
    Move(#[from] InnerImapMessageMoveError),
    #[error("IMAP message move was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`ImapMessageMove::resume`].
#[derive(Debug)]
pub enum ImapMessageMoveResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapMessageMoveError),
}

enum State {
    Selecting {
        select: ImapMailboxSelect,
        sequence_set: SequenceSet,
        target: ImapMailbox<'static>,
        uid: bool,
    },
    Moving(InnerImapMessageMove),
    Done,
}

/// I/O-free coroutine wrapping `SELECT <from>` followed by `UID MOVE
/// <ids> <to>`. UIDs by default; pass `uid = false` to interpret the
/// sequence-set as message sequence numbers.
pub struct ImapMessageMove {
    state: State,
}

impl ImapMessageMove {
    pub fn new(
        context: ImapContext,
        from: ImapMailbox<'static>,
        to: ImapMailbox<'static>,
        sequence_set: SequenceSet,
        uid: bool,
    ) -> Self {
        trace!("prepare IMAP message move");
        Self {
            state: State::Selecting {
                select: ImapMailboxSelect::new(context, from),
                sequence_set,
                target: to,
                uid,
            },
        }
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> ImapMessageMoveResult {
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
                        return ImapMessageMoveResult::WantsRead;
                    }
                    ImapMailboxSelectResult::WantsWrite(bytes) => {
                        self.state = State::Selecting {
                            select,
                            sequence_set,
                            target,
                            uid,
                        };
                        return ImapMessageMoveResult::WantsWrite(bytes);
                    }
                    ImapMailboxSelectResult::Err { err, .. } => {
                        return ImapMessageMoveResult::Err(err.into());
                    }
                    ImapMailboxSelectResult::Ok { context, .. } => {
                        let mv = InnerImapMessageMove::new(context, sequence_set, target, uid);
                        self.state = State::Moving(mv);
                    }
                },
                State::Moving(mut mv) => match mv.resume(arg.take()) {
                    InnerImapMessageMoveResult::WantsRead => {
                        self.state = State::Moving(mv);
                        return ImapMessageMoveResult::WantsRead;
                    }
                    InnerImapMessageMoveResult::WantsWrite(bytes) => {
                        self.state = State::Moving(mv);
                        return ImapMessageMoveResult::WantsWrite(bytes);
                    }
                    InnerImapMessageMoveResult::Err { err, .. } => {
                        return ImapMessageMoveResult::Err(err.into());
                    }
                    InnerImapMessageMoveResult::Ok { .. } => return ImapMessageMoveResult::Ok,
                },
                State::Done => {
                    return ImapMessageMoveResult::Err(ImapMessageMoveError::AlreadyDone);
                }
            }
        }
    }
}
