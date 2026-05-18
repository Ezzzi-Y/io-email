//! IMAP mailbox listing (`LIST "" "*"`), wrapping
//! [`io_imap::rfc3501::list::ImapMailboxList`].
//!
//! Counts are not populated; IMAP requires a separate STATUS round-trip
//! per mailbox, which is driven at the [`EmailClientStd`] level.
//!
//! [`EmailClientStd`]: crate::client::EmailClientStd

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use io_imap::{
    context::ImapContext,
    rfc3501::list::{
        ImapMailboxList as InnerImapMailboxList, ImapMailboxListError,
        ImapMailboxListResult as InnerImapMailboxListResult,
    },
    types::{
        core::QuotedChar,
        flag::FlagNameAttribute,
        mailbox::{ListMailbox, Mailbox as InnerImapMailbox},
    },
};
use log::trace;

use crate::mailbox::Mailbox;

/// Result returned by [`ImapMailboxList::resume`].
#[derive(Debug)]
pub enum ImapMailboxListResult {
    Ok(Vec<Mailbox>),
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(ImapMailboxListError),
}

/// I/O-free coroutine listing every IMAP mailbox visible to the session.
pub struct ImapMailboxList {
    inner: InnerImapMailboxList,
}

impl ImapMailboxList {
    pub fn new(context: ImapContext) -> Self {
        trace!("prepare IMAP mailbox listing");
        // SAFETY: "" and "*" are always valid IMAP mailbox tokens.
        let reference: InnerImapMailbox<'static> = "".try_into().unwrap();
        let pattern: ListMailbox<'static> = "*".try_into().unwrap();
        Self {
            inner: InnerImapMailboxList::new(context, reference, pattern),
        }
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> ImapMailboxListResult {
        match self.inner.resume(arg) {
            InnerImapMailboxListResult::WantsRead => ImapMailboxListResult::WantsRead,
            InnerImapMailboxListResult::WantsWrite(bytes) => {
                ImapMailboxListResult::WantsWrite(bytes)
            }
            InnerImapMailboxListResult::Ok { mailboxes, .. } => {
                let mailboxes = mailboxes.into_iter().map(Mailbox::from).collect();
                ImapMailboxListResult::Ok(mailboxes)
            }
            InnerImapMailboxListResult::Err { err, .. } => ImapMailboxListResult::Err(err),
        }
    }
}

impl
    From<(
        InnerImapMailbox<'static>,
        Option<QuotedChar>,
        Vec<FlagNameAttribute<'static>>,
    )> for Mailbox
{
    fn from(
        (mailbox, _delimiter, _attrs): (
            InnerImapMailbox<'static>,
            Option<QuotedChar>,
            Vec<FlagNameAttribute<'static>>,
        ),
    ) -> Self {
        let name = match mailbox {
            InnerImapMailbox::Inbox => "Inbox".to_string(),
            InnerImapMailbox::Other(other) => {
                String::from_utf8_lossy(other.inner().as_ref()).into_owned()
            }
        };

        Self {
            id: name.clone(),
            name,
            total: None,
            unread: None,
        }
    }
}
