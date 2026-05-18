//! Maildir message get, wrapping
//! [`io_maildir::coroutines::message_get::MaildirMessageGet`]. Returns
//! raw RFC 5322 bytes.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

use io_maildir::{
    coroutines::message_get::{
        MaildirMessageGet as InnerMaildirMessageGet, MaildirMessageGetArg as InnerArg,
        MaildirMessageGetError, MaildirMessageGetResult as InnerResult,
    },
    maildir::Maildir,
};
use log::trace;

/// Argument fed back to [`MaildirMessageGet::resume`].
#[derive(Debug)]
pub enum MaildirMessageGetArg {
    DirRead(BTreeMap<String, BTreeSet<String>>),
    FileRead(BTreeMap<String, Vec<u8>>),
}

/// Result returned by [`MaildirMessageGet::resume`].
#[derive(Debug)]
pub enum MaildirMessageGetResult {
    Ok(Vec<u8>),
    WantsDirRead(BTreeSet<String>),
    WantsFileRead(BTreeSet<String>),
    Err(MaildirMessageGetError),
}

/// I/O-free coroutine reading a single Maildir message.
pub struct MaildirMessageGet {
    inner: InnerMaildirMessageGet,
}

impl MaildirMessageGet {
    pub fn new(maildir: Maildir, id: impl ToString) -> Self {
        trace!("prepare Maildir message get");
        Self {
            inner: InnerMaildirMessageGet::new(maildir, id),
        }
    }

    pub fn resume(&mut self, arg: Option<MaildirMessageGetArg>) -> MaildirMessageGetResult {
        let inner_arg = arg.map(|arg| match arg {
            MaildirMessageGetArg::DirRead(entries) => InnerArg::DirRead(entries),
            MaildirMessageGetArg::FileRead(contents) => InnerArg::FileRead(contents),
        });

        match self.inner.resume(inner_arg) {
            InnerResult::WantsDirRead(paths) => MaildirMessageGetResult::WantsDirRead(paths),
            InnerResult::WantsFileRead(paths) => MaildirMessageGetResult::WantsFileRead(paths),
            InnerResult::Ok(message) => MaildirMessageGetResult::Ok(message.into()),
            InnerResult::Err(err) => MaildirMessageGetResult::Err(err),
        }
    }
}
