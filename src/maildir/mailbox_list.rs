//! Maildir list-mailboxes coroutine.
//!
//! Wraps [`io_maildir::maildir::list::MaildirList`]: scans the store
//! root and probes each candidate child for the `cur/` + `new/` +
//! `tmp/` triad. The layout (fs vs Maildir++) is read from the
//! [`MaildirStore`] passed at construction time.
//!
//! `with_counts` is currently a no-op: surfacing totals/unread would
//! need a follow-up directory walk over each maildir's `cur/` + `new/`
//! plus filename-flag parsing. Wire that in once io-email grows a
//! dedicated MaildirMailboxStatus coroutine and add it as a chained
//! stage here.
//!
//! Emits the shared [`Mailbox`] shape directly; Maildir-specific data
//! (root path metadata, subdirectory layout) is dropped on purpose to
//! stay LCD.

use alloc::{string::ToString, vec::Vec};

use io_maildir::{
    coroutine::*,
    maildir::{
        list::{MaildirList as InnerMaildirList, MaildirListError},
        types::Maildir,
    },
    store::MaildirStore,
};
use log::trace;
use thiserror::Error;

use crate::mailbox::Mailbox;

/// Errors produced by [`MaildirMailboxList`].
#[derive(Debug, Error)]
pub enum MaildirMailboxListError {
    #[error(transparent)]
    List(#[from] MaildirListError),
}

/// I/O-free coroutine listing every Maildir under the store root.
pub struct MaildirMailboxList {
    inner: InnerMaildirList,
}

impl MaildirMailboxList {
    /// `MaildirList` against `store`'s configured layout.
    ///
    /// `_with_counts` is accepted for symmetry with the other backends
    /// but currently ignored; see the module doc for the path to
    /// surfacing counts.
    pub fn new(store: &MaildirStore, _with_counts: bool) -> Self {
        trace!(
            "prepare Maildir mailbox listing (maildirpp={})",
            store.maildirpp
        );
        Self {
            inner: InnerMaildirList::new(store),
        }
    }
}

impl MaildirCoroutine for MaildirMailboxList {
    type Yield = MaildirYield;
    type Return = Result<Vec<Mailbox>, MaildirMailboxListError>;

    fn resume(
        &mut self,
        arg: Option<MaildirReply>,
    ) -> MaildirCoroutineState<Self::Yield, Self::Return> {
        match self.inner.resume(arg) {
            MaildirCoroutineState::Yielded(y) => MaildirCoroutineState::Yielded(y),
            MaildirCoroutineState::Complete(Ok(maildirs)) => {
                let mut mailboxes: Vec<Mailbox> = maildirs.into_iter().map(mailbox_from).collect();
                mailboxes.sort_by(|a, b| a.name.cmp(&b.name));
                MaildirCoroutineState::Complete(Ok(mailboxes))
            }
            MaildirCoroutineState::Complete(Err(err)) => {
                MaildirCoroutineState::Complete(Err(err.into()))
            }
        }
    }
}

/// Converts one [`Maildir`] into the shared [`Mailbox`] shape.
///
/// `id` is the on-disk path so downstream ops can locate the maildir;
/// `name` is the last path segment (Maildir++ dotted names are kept
/// verbatim: decoding is the caller's responsibility for now).
/// Counts default to `None`; populating them needs the follow-up walk
/// described in the module doc.
fn mailbox_from(maildir: Maildir) -> Mailbox {
    let name = maildir.name().unwrap_or("").to_string();
    let id = maildir.path().to_string();
    Mailbox {
        id,
        name,
        total: None,
        unread: None,
    }
}
