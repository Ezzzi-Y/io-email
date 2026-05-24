//! m2dir backend implementations of the [`EmailClientStd`] private
//! dispatch methods.

use alloc::{
    collections::BTreeSet,
    string::{String, ToString},
    vec::Vec,
};

use io_m2dir::{entry::M2dirFullEntry, flag::M2dirFlags as M2Flags};
use mail_parser::MessageParser;

use crate::{
    client::{EmailClientStd, EmailClientStdError},
    envelope::Envelope,
    flag::{Flag, FlagOp},
    m2dir::convert::{envelope_from, flag_to_meta_line, mailbox_from, open_m2dir, paginate},
    mailbox::Mailbox,
};

impl EmailClientStd {
    /// Registers the m2dir backend. See [`Self::with_imap`] for the
    /// ordering rule.
    pub fn with_m2dir(mut self, client: io_m2dir::client::M2dirClient) -> Self {
        self.m2dir = Some(client);

        if !self.order.contains(&crate::client::BackendKind::M2dir) {
            self.order.push(crate::client::BackendKind::M2dir);
        }

        self
    }

    /// Borrows the underlying m2dir client when registered. The
    /// shared dispatcher has no diff equivalent for filesystem-backed
    /// stores: callers needing change detection (sync) build their
    /// own manifest format on top of this and stay protocol-agnostic
    /// for the IMAP / JMAP arms.
    pub fn as_m2dir(&self) -> Option<&io_m2dir::client::M2dirClient> {
        self.m2dir.as_ref()
    }

    /// Mutable variant of [`Self::as_m2dir`].
    pub fn as_m2dir_mut(&mut self) -> Option<&mut io_m2dir::client::M2dirClient> {
        self.m2dir.as_mut()
    }

    pub(crate) fn m2dir_list_mailboxes(&mut self) -> Result<Vec<Mailbox>, EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");
        let m2dirs = client.list_mailboxes()?;
        let mut mailboxes: Vec<_> = m2dirs.iter().map(mailbox_from).collect();
        mailboxes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(mailboxes)
    }

    pub(crate) fn m2dir_list_envelopes_par(
        &mut self,
        mailbox: &str,
        page: Option<u32>,
        page_size: Option<u32>,
        with_attachment: bool,
    ) -> Result<Vec<Envelope>, EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");

        let m2dir = open_m2dir(client, mailbox)?;
        let entries = client.list_entries(m2dir.clone())?;
        let loaded = client.read_entries_par(&m2dir, &entries)?;
        let envelopes = build_envelopes(&loaded, with_attachment)?;
        Ok(paginate(envelopes, page, page_size))
    }

    pub(crate) fn m2dir_store_flags(
        &mut self,
        mailbox: &str,
        ids: &[&str],
        flags: &[Flag],
        op: FlagOp,
    ) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");

        let m2dir = open_m2dir(client, mailbox)?;
        let meta_flags: M2Flags = flags.iter().map(flag_to_meta_line).collect();

        for id in ids {
            match op {
                FlagOp::Add => {
                    client.add_flags(&m2dir, *id, meta_flags.clone())?;
                }
                FlagOp::Set => {
                    client.set_flags(&m2dir, *id, meta_flags.clone())?;
                }
                FlagOp::Remove => {
                    client.remove_flags(&m2dir, *id, meta_flags.clone())?;
                }
            }
        }

        Ok(())
    }

    pub(crate) fn m2dir_get_message(
        &mut self,
        mailbox: &str,
        id: &str,
    ) -> Result<Vec<u8>, EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");
        let m2dir = open_m2dir(client, mailbox)?;
        let (_, bytes) = client.get(m2dir, id)?;
        Ok(bytes)
    }

    pub(crate) fn m2dir_add_message(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        raw: Vec<u8>,
    ) -> Result<String, EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");

        let m2dir = open_m2dir(client, mailbox)?;
        let entry = client.store(m2dir.clone(), raw)?;
        let id = entry.id().to_string();

        if !flags.is_empty() {
            let meta_flags: M2Flags = flags.iter().map(flag_to_meta_line).collect();
            client.set_flags(&m2dir, &id, meta_flags)?;
        }

        Ok(id)
    }

    pub(crate) fn m2dir_create_mailbox(&mut self, name: &str) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");
        let _ = client.create_mailbox(name)?;
        Ok(())
    }

    pub(crate) fn m2dir_delete_mailbox(&mut self, name: &str) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");
        let m2dir = open_m2dir(client, name)?;
        client.delete_mailbox(m2dir.path().clone())?;
        Ok(())
    }

    pub(crate) fn m2dir_delete_message(
        &mut self,
        mailbox: &str,
        id: &str,
    ) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");
        let m2dir = open_m2dir(client, mailbox)?;
        client.delete_message(m2dir, id)?;
        Ok(())
    }

    pub(crate) fn m2dir_copy_messages(
        &mut self,
        from: &str,
        to: &str,
        ids: &[&str],
    ) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");

        let src = open_m2dir(client, from)?;
        let dst = open_m2dir(client, to)?;

        for id in ids {
            let (_, bytes) = client.get(src.clone(), *id)?;
            let flags = client.read_flags(&src, *id)?;
            let entry = client.store(dst.clone(), bytes)?;
            if !flags.is_empty() {
                client.set_flags(&dst, entry.id(), flags)?;
            }
        }

        Ok(())
    }

    pub(crate) fn m2dir_move_messages(
        &mut self,
        from: &str,
        to: &str,
        ids: &[&str],
    ) -> Result<(), EmailClientStdError> {
        let client = self.m2dir.as_mut().expect("m2dir slot registered");

        let src = open_m2dir(client, from)?;
        let dst = open_m2dir(client, to)?;

        for id in ids {
            let (_, bytes) = client.get(src.clone(), *id)?;
            let flags = client.read_flags(&src, *id)?;
            let entry = client.store(dst.clone(), bytes)?;
            if !flags.is_empty() {
                client.set_flags(&dst, entry.id(), flags)?;
            }
            client.delete_message(src.clone(), *id)?;
        }

        Ok(())
    }
}

fn build_envelopes(
    loaded: &BTreeSet<M2dirFullEntry>,
    with_attachment: bool,
) -> Result<Vec<Envelope>, EmailClientStdError> {
    let parser = MessageParser::default();
    let mut envelopes: Vec<Envelope> = Vec::with_capacity(loaded.len());

    for full in loaded {
        // Header-only parse when attachment detection is not
        // requested: skips body decoding (quoted-printable, base64,
        // MIME tree walk) entirely. Subject / from / to / date come
        // from headers; `size` is the raw byte length.
        let parsed = if with_attachment {
            parser.parse(full.contents())
        } else {
            parser.parse_headers(full.contents())
        }
        .ok_or(EmailClientStdError::OperationFailed("parse m2dir message"))?;

        let mut envelope = envelope_from(full.entry(), full.flags(), &parsed);
        if with_attachment {
            envelope.has_attachment = Some(parsed.attachment_count() > 0);
        }
        envelopes.push(envelope);
    }

    envelopes.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(envelopes)
}
