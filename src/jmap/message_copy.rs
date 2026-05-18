//! JMAP message copy (`Mailbox/query` + `Email/set`), wrapping a
//! private orchestrator. Single-account, in-place copy: patches each
//! email's `mailboxIds` to add the destination.

use core::mem;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::{
        email_set::{JmapEmailSet, JmapEmailSetArgs, JmapEmailSetError, JmapEmailSetResult},
        mailbox::Mailbox,
        mailbox_query::{JmapMailboxQuery, JmapMailboxQueryError, JmapMailboxQueryResult},
    },
};
use log::trace;
use secrecy::SecretString;
use thiserror::Error;

/// Errors produced while orchestrating Mailbox lookup + Email/set for
/// JMAP message copy.
#[derive(Debug, Error)]
pub enum JmapMessageCopyError {
    #[error(transparent)]
    MailboxQuery(#[from] JmapMailboxQueryError),
    #[error(transparent)]
    EmailSet(#[from] JmapEmailSetError),
    #[error("no JMAP mailbox matched the name {0:?}")]
    UnknownMailbox(String),
    #[error("JMAP message copy was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`JmapMessageCopy::resume`].
#[derive(Debug)]
pub enum JmapMessageCopyResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapMessageCopyError),
}

enum State {
    Resolving {
        query: JmapMailboxQuery,
        session: JmapSession,
        http_auth: SecretString,
        ids: Vec<String>,
        to_name: String,
    },
    Setting(JmapEmailSet),
    Done,
}

/// I/O-free orchestrator: `Mailbox/query` to resolve the destination
/// name to an id, then `Email/set` to add every email to that mailbox.
pub struct JmapMessageCopy {
    state: State,
}

impl JmapMessageCopy {
    pub fn new(
        session: &JmapSession,
        http_auth: &SecretString,
        ids: impl IntoIterator<Item = String>,
        to_name: impl ToString,
    ) -> Result<Self, JmapMessageCopyError> {
        trace!("prepare JMAP message copy");
        let query = JmapMailboxQuery::new(session, http_auth, None, None, None, None, None)?;
        Ok(Self {
            state: State::Resolving {
                query,
                session: session.clone(),
                http_auth: http_auth.clone(),
                ids: ids.into_iter().collect(),
                to_name: to_name.to_string(),
            },
        })
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> JmapMessageCopyResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Resolving {
                    mut query,
                    session,
                    http_auth,
                    ids,
                    to_name,
                } => match query.resume(arg.take()) {
                    JmapMailboxQueryResult::WantsRead => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            ids,
                            to_name,
                        };
                        return JmapMessageCopyResult::WantsRead;
                    }
                    JmapMailboxQueryResult::WantsWrite(bytes) => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            ids,
                            to_name,
                        };
                        return JmapMessageCopyResult::WantsWrite(bytes);
                    }
                    JmapMailboxQueryResult::Err(err) => {
                        return JmapMessageCopyResult::Err(err.into());
                    }
                    JmapMailboxQueryResult::Ok { mailboxes, .. } => {
                        let Some(to_id) = find_mailbox_id(&mailboxes, &to_name) else {
                            return JmapMessageCopyResult::Err(
                                JmapMessageCopyError::UnknownMailbox(to_name),
                            );
                        };

                        let mut args = JmapEmailSetArgs::default();
                        for id in &ids {
                            args.add_to_mailbox(id.clone(), to_id.clone());
                        }

                        let set = match JmapEmailSet::new(&session, &http_auth, args) {
                            Ok(s) => s,
                            Err(err) => return JmapMessageCopyResult::Err(err.into()),
                        };
                        self.state = State::Setting(set);
                    }
                },
                State::Setting(mut set) => match set.resume(arg.take()) {
                    JmapEmailSetResult::WantsRead => {
                        self.state = State::Setting(set);
                        return JmapMessageCopyResult::WantsRead;
                    }
                    JmapEmailSetResult::WantsWrite(bytes) => {
                        self.state = State::Setting(set);
                        return JmapMessageCopyResult::WantsWrite(bytes);
                    }
                    JmapEmailSetResult::Err(err) => return JmapMessageCopyResult::Err(err.into()),
                    JmapEmailSetResult::Ok { .. } => return JmapMessageCopyResult::Ok,
                },
                State::Done => {
                    return JmapMessageCopyResult::Err(JmapMessageCopyError::AlreadyDone);
                }
            }
        }
    }
}

pub(crate) fn find_mailbox_id(mailboxes: &[Mailbox], name: &str) -> Option<String> {
    mailboxes
        .iter()
        .find(|m| m.name.as_deref() == Some(name))
        .and_then(|m| m.id.clone())
}
