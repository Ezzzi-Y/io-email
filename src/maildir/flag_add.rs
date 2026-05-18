//! Maildir flag add, wrapping
//! [`io_maildir::coroutines::flags_add::MaildirFlagsAdd`].

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use io_maildir::{
    coroutines::flags_add::{
        MaildirFlagsAdd as InnerMaildirFlagsAdd, MaildirFlagsAddArg, MaildirFlagsAddError,
    },
    flag::Flags,
    maildir::Maildir,
};
use log::trace;

/// Argument fed back to [`MaildirFlagAdd::resume`].
#[derive(Debug)]
pub enum MaildirFlagAddArg {
    DirRead(BTreeMap<String, BTreeSet<String>>),
    Rename,
}

/// Result returned by [`MaildirFlagAdd::resume`].
#[derive(Debug)]
pub enum MaildirFlagAddResult {
    Ok,
    WantsDirRead(BTreeSet<String>),
    WantsRename(Vec<(String, String)>),
    Err(MaildirFlagsAddError),
}

/// I/O-free coroutine adding flags to a single Maildir message.
pub struct MaildirFlagAdd {
    inner: InnerMaildirFlagsAdd,
}

impl MaildirFlagAdd {
    pub fn new(maildir: Maildir, id: impl ToString, flags: Flags) -> Self {
        trace!("prepare Maildir flag add");
        Self {
            inner: InnerMaildirFlagsAdd::new(maildir, id, flags),
        }
    }

    pub fn resume(&mut self, arg: Option<MaildirFlagAddArg>) -> MaildirFlagAddResult {
        use io_maildir::coroutines::flags_add::MaildirFlagsAddResult;

        let inner_arg = arg.map(|arg| match arg {
            MaildirFlagAddArg::DirRead(entries) => MaildirFlagsAddArg::DirRead(entries),
            MaildirFlagAddArg::Rename => MaildirFlagsAddArg::Rename,
        });

        match self.inner.resume(inner_arg) {
            MaildirFlagsAddResult::WantsDirRead(paths) => MaildirFlagAddResult::WantsDirRead(paths),
            MaildirFlagsAddResult::WantsRename(pairs) => MaildirFlagAddResult::WantsRename(pairs),
            MaildirFlagsAddResult::Ok => MaildirFlagAddResult::Ok,
            MaildirFlagsAddResult::Err(err) => MaildirFlagAddResult::Err(err),
        }
    }
}
