//! Maildir flag delete, wrapping
//! [`io_maildir::coroutines::flags_remove::MaildirFlagsRemove`].

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use io_maildir::{
    coroutines::flags_remove::{
        MaildirFlagsRemove as InnerMaildirFlagsRemove, MaildirFlagsRemoveArg,
        MaildirFlagsRemoveError,
    },
    flag::Flags,
    maildir::Maildir,
};
use log::trace;

/// Argument fed back to [`MaildirFlagDelete::resume`].
#[derive(Debug)]
pub enum MaildirFlagDeleteArg {
    DirRead(BTreeMap<String, BTreeSet<String>>),
    Rename,
}

/// Result returned by [`MaildirFlagDelete::resume`].
#[derive(Debug)]
pub enum MaildirFlagDeleteResult {
    Ok,
    WantsDirRead(BTreeSet<String>),
    WantsRename(Vec<(String, String)>),
    Err(MaildirFlagsRemoveError),
}

/// I/O-free coroutine removing flags from a single Maildir message.
pub struct MaildirFlagDelete {
    inner: InnerMaildirFlagsRemove,
}

impl MaildirFlagDelete {
    pub fn new(maildir: Maildir, id: impl ToString, flags: Flags) -> Self {
        trace!("prepare Maildir flag delete");
        Self {
            inner: InnerMaildirFlagsRemove::new(maildir, id, flags),
        }
    }

    pub fn resume(&mut self, arg: Option<MaildirFlagDeleteArg>) -> MaildirFlagDeleteResult {
        use io_maildir::coroutines::flags_remove::MaildirFlagsRemoveResult;

        let inner_arg = arg.map(|arg| match arg {
            MaildirFlagDeleteArg::DirRead(entries) => MaildirFlagsRemoveArg::DirRead(entries),
            MaildirFlagDeleteArg::Rename => MaildirFlagsRemoveArg::Rename,
        });

        match self.inner.resume(inner_arg) {
            MaildirFlagsRemoveResult::WantsDirRead(paths) => {
                MaildirFlagDeleteResult::WantsDirRead(paths)
            }
            MaildirFlagsRemoveResult::WantsRename(pairs) => {
                MaildirFlagDeleteResult::WantsRename(pairs)
            }
            MaildirFlagsRemoveResult::Ok => MaildirFlagDeleteResult::Ok,
            MaildirFlagsRemoveResult::Err(err) => MaildirFlagDeleteResult::Err(err),
        }
    }
}
