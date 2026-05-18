//! JMAP mailbox listing (`Mailbox/query` + `Mailbox/get`), wrapping
//! [`JmapMailboxQuery`] and producing shared [`Mailbox`] values.
//!
//! `total`/`unread` are populated unconditionally; JMAP returns them in
//! the same `Mailbox/get` response.

use alloc::vec::Vec;

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::{
        mailbox::Mailbox as JmapMailbox,
        mailbox_query::{JmapMailboxQuery, JmapMailboxQueryError, JmapMailboxQueryResult},
    },
};
use log::trace;
use secrecy::SecretString;

use crate::mailbox::Mailbox;

/// Result returned by [`JmapMailboxList::resume`].
#[derive(Debug)]
pub enum JmapMailboxListResult {
    Ok(Vec<Mailbox>),
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapMailboxQueryError),
}

/// I/O-free coroutine listing every JMAP mailbox in the session's
/// primary mail account.
pub struct JmapMailboxList {
    inner: JmapMailboxQuery,
}

impl JmapMailboxList {
    pub fn new(
        session: &JmapSession,
        http_auth: &SecretString,
    ) -> Result<Self, JmapMailboxQueryError> {
        trace!("prepare JMAP mailbox listing");
        let inner = JmapMailboxQuery::new(session, http_auth, None, None, None, None, None)?;
        Ok(Self { inner })
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> JmapMailboxListResult {
        match self.inner.resume(arg) {
            JmapMailboxQueryResult::WantsRead => JmapMailboxListResult::WantsRead,
            JmapMailboxQueryResult::WantsWrite(bytes) => JmapMailboxListResult::WantsWrite(bytes),
            JmapMailboxQueryResult::Ok { mailboxes, .. } => {
                let mailboxes = mailboxes.into_iter().map(Mailbox::from).collect();
                JmapMailboxListResult::Ok(mailboxes)
            }
            JmapMailboxQueryResult::Err(err) => JmapMailboxListResult::Err(err),
        }
    }
}

impl From<JmapMailbox> for Mailbox {
    fn from(mbox: JmapMailbox) -> Self {
        Self {
            id: mbox.id.unwrap_or_default(),
            name: mbox.name.unwrap_or_default(),
            total: Some(u64::from(mbox.total_emails)),
            unread: Some(u64::from(mbox.unread_emails)),
        }
    }
}
