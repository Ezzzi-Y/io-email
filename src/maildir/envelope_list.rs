//! Maildir envelope listing, wrapping
//! [`io_maildir::coroutines::message_list::MaildirMessagesList`].
//!
//! Maildir has no inherent ordering; envelopes are sorted by date
//! descending and then paginated. `page` is 1-indexed.

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};
use std::path::Path;

use chrono::DateTime;
use io_maildir::{
    coroutines::message_list::{
        MaildirMessagesList as InnerMaildirMessagesList, MaildirMessagesListArg,
        MaildirMessagesListError,
    },
    maildir::Maildir,
    message::Message as MaildirMessage,
    parser::Address as MailParserAddress,
};
use log::trace;

use crate::{address::Address, envelope::Envelope, flag::Flag};

/// Argument fed back to [`MaildirEnvelopeList::resume`].
#[derive(Debug)]
pub enum MaildirEnvelopeListArg {
    DirRead(BTreeMap<String, BTreeSet<String>>),
    FileRead(BTreeMap<String, Vec<u8>>),
}

/// Result returned by [`MaildirEnvelopeList::resume`].
#[derive(Debug)]
pub enum MaildirEnvelopeListResult {
    Ok(Vec<Envelope>),
    WantsDirRead(BTreeSet<String>),
    WantsFileRead(BTreeSet<String>),
    Err(MaildirMessagesListError),
}

/// I/O-free coroutine listing every message inside a single Maildir,
/// sorted by date descending then paginated.
pub struct MaildirEnvelopeList {
    inner: InnerMaildirMessagesList,
    page: Option<u32>,
    page_size: Option<u32>,
}

impl MaildirEnvelopeList {
    pub fn new(maildir: Maildir, page: Option<u32>, page_size: Option<u32>) -> Self {
        trace!("prepare Maildir envelope listing");
        Self {
            inner: InnerMaildirMessagesList::new(maildir),
            page,
            page_size,
        }
    }

    pub fn resume(&mut self, arg: Option<MaildirEnvelopeListArg>) -> MaildirEnvelopeListResult {
        use io_maildir::coroutines::message_list::MaildirMessagesListResult;

        let inner_arg = arg.map(|arg| match arg {
            MaildirEnvelopeListArg::DirRead(entries) => MaildirMessagesListArg::DirRead(entries),
            MaildirEnvelopeListArg::FileRead(contents) => {
                MaildirMessagesListArg::FileRead(contents)
            }
        });

        match self.inner.resume(inner_arg) {
            MaildirMessagesListResult::WantsDirRead(paths) => {
                MaildirEnvelopeListResult::WantsDirRead(paths)
            }
            MaildirMessagesListResult::WantsFileRead(paths) => {
                MaildirEnvelopeListResult::WantsFileRead(paths)
            }
            MaildirMessagesListResult::Ok(messages) => {
                let mut envelopes: Vec<Envelope> =
                    messages.into_iter().map(Envelope::from).collect();
                envelopes.sort_by(|a, b| b.date.cmp(&a.date));
                MaildirEnvelopeListResult::Ok(paginate(envelopes, self.page, self.page_size))
            }
            MaildirMessagesListResult::Err(err) => MaildirEnvelopeListResult::Err(err),
        }
    }
}

impl From<MaildirMessage> for Envelope {
    fn from(message: MaildirMessage) -> Self {
        let id = message.id().unwrap_or_default().to_string();
        let flags = parse_filename_flags(message.path());
        let size = message.contents().len() as u64;

        let parsed = message.parsed();

        let subject = parsed
            .as_ref()
            .and_then(|m| m.subject())
            .unwrap_or_default()
            .to_string();

        let from = parsed
            .as_ref()
            .and_then(|m| m.from())
            .map(addresses_from)
            .unwrap_or_default();

        let to = parsed
            .as_ref()
            .and_then(|m| m.to())
            .map(addresses_from)
            .unwrap_or_default();

        let date = parsed
            .as_ref()
            .and_then(|m| m.date())
            .and_then(|d| DateTime::parse_from_rfc3339(&d.to_rfc3339()).ok());

        let has_attachment = parsed.as_ref().map(|m| m.attachment_count() > 0);

        Self {
            id,
            flags,
            subject,
            from,
            to,
            date,
            size,
            has_attachment,
        }
    }
}

fn paginate(envelopes: Vec<Envelope>, page: Option<u32>, page_size: Option<u32>) -> Vec<Envelope> {
    let Some(size) = page_size else {
        return envelopes;
    };

    if size == 0 {
        return Vec::new();
    }

    let page = page.unwrap_or(1).max(1);
    let skip = ((page - 1) as usize).saturating_mul(size as usize);

    if skip >= envelopes.len() {
        return Vec::new();
    }

    envelopes
        .into_iter()
        .skip(skip)
        .take(size as usize)
        .collect()
}

fn parse_filename_flags(path: &Path) -> BTreeSet<Flag> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return BTreeSet::new();
    };

    let Some((_, letters)) = name.rsplit_once(',') else {
        return BTreeSet::new();
    };

    letters
        .chars()
        .filter_map(|c| {
            let mut buf = [0u8; 4];
            Flag::parse(c.encode_utf8(&mut buf))
        })
        .collect()
}

fn addresses_from(addrs: &MailParserAddress<'_>) -> Vec<Address> {
    addrs
        .clone()
        .into_list()
        .into_iter()
        .filter_map(|a| {
            let email = a.address?.into_owned();
            if email.is_empty() {
                return None;
            }
            let name = a.name.map(|s| s.into_owned());
            Some(Address { name, email })
        })
        .collect()
}
