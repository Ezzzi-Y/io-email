//! JMAP message move (`Mailbox/query` + `Email/set`), wrapping a
//! private orchestrator. Resolves source and destination mailbox names
//! to ids, then patches each email's `mailboxIds` to remove the source
//! and add the destination.

use core::mem;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::{
        email_set::{JmapEmailSet, JmapEmailSetArgs, JmapEmailSetError, JmapEmailSetResult},
        mailbox_query::{JmapMailboxQuery, JmapMailboxQueryError, JmapMailboxQueryResult},
    },
};
use log::trace;
use secrecy::SecretString;
use thiserror::Error;

use crate::jmap::message_copy::find_mailbox_id;

/// Errors produced while orchestrating Mailbox lookup + Email/set for
/// JMAP message move.
#[derive(Debug, Error)]
pub enum JmapMessageMoveError {
    #[error(transparent)]
    MailboxQuery(#[from] JmapMailboxQueryError),
    #[error(transparent)]
    EmailSet(#[from] JmapEmailSetError),
    #[error("no JMAP mailbox matched the name {0:?}")]
    UnknownMailbox(String),
    #[error("JMAP message move was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`JmapMessageMove::resume`].
#[derive(Debug)]
pub enum JmapMessageMoveResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapMessageMoveError),
}

enum State {
    Resolving {
        query: JmapMailboxQuery,
        session: JmapSession,
        http_auth: SecretString,
        ids: Vec<String>,
        from_name: String,
        to_name: String,
    },
    Setting(JmapEmailSet),
    Done,
}

/// I/O-free orchestrator that resolves source and destination mailbox
/// names to ids, then issues `Email/set` patches removing each email
/// from the source mailbox and adding it to the destination.
pub struct JmapMessageMove {
    state: State,
}

impl JmapMessageMove {
    pub fn new(
        session: &JmapSession,
        http_auth: &SecretString,
        ids: impl IntoIterator<Item = String>,
        from_name: impl ToString,
        to_name: impl ToString,
    ) -> Result<Self, JmapMessageMoveError> {
        trace!("prepare JMAP message move");
        let query = JmapMailboxQuery::new(session, http_auth, None, None, None, None, None)?;
        Ok(Self {
            state: State::Resolving {
                query,
                session: session.clone(),
                http_auth: http_auth.clone(),
                ids: ids.into_iter().collect(),
                from_name: from_name.to_string(),
                to_name: to_name.to_string(),
            },
        })
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> JmapMessageMoveResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Resolving {
                    mut query,
                    session,
                    http_auth,
                    ids,
                    from_name,
                    to_name,
                } => match query.resume(arg.take()) {
                    JmapMailboxQueryResult::WantsRead => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            ids,
                            from_name,
                            to_name,
                        };
                        return JmapMessageMoveResult::WantsRead;
                    }
                    JmapMailboxQueryResult::WantsWrite(bytes) => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            ids,
                            from_name,
                            to_name,
                        };
                        return JmapMessageMoveResult::WantsWrite(bytes);
                    }
                    JmapMailboxQueryResult::Err(err) => {
                        return JmapMessageMoveResult::Err(err.into());
                    }
                    JmapMailboxQueryResult::Ok { mailboxes, .. } => {
                        let Some(from_id) = find_mailbox_id(&mailboxes, &from_name) else {
                            return JmapMessageMoveResult::Err(
                                JmapMessageMoveError::UnknownMailbox(from_name),
                            );
                        };
                        let Some(to_id) = find_mailbox_id(&mailboxes, &to_name) else {
                            return JmapMessageMoveResult::Err(
                                JmapMessageMoveError::UnknownMailbox(to_name),
                            );
                        };

                        let mut args = JmapEmailSetArgs::default();
                        for id in &ids {
                            args.remove_from_mailbox(id.clone(), from_id.clone());
                            args.add_to_mailbox(id.clone(), to_id.clone());
                        }

                        let set = match JmapEmailSet::new(&session, &http_auth, args) {
                            Ok(s) => s,
                            Err(err) => return JmapMessageMoveResult::Err(err.into()),
                        };
                        self.state = State::Setting(set);
                    }
                },
                State::Setting(mut set) => match set.resume(arg.take()) {
                    JmapEmailSetResult::WantsRead => {
                        self.state = State::Setting(set);
                        return JmapMessageMoveResult::WantsRead;
                    }
                    JmapEmailSetResult::WantsWrite(bytes) => {
                        self.state = State::Setting(set);
                        return JmapMessageMoveResult::WantsWrite(bytes);
                    }
                    JmapEmailSetResult::Err(err) => return JmapMessageMoveResult::Err(err.into()),
                    JmapEmailSetResult::Ok { .. } => return JmapMessageMoveResult::Ok,
                },
                State::Done => {
                    return JmapMessageMoveResult::Err(JmapMessageMoveError::AlreadyDone);
                }
            }
        }
    }
}
