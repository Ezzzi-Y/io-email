//! JMAP message add (`Blob/upload` → `Mailbox/query` → `Email/import`),
//! wrapping a private orchestrator that uploads raw bytes, resolves the
//! destination mailbox name to an id, then imports the blob.

use core::mem;

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

use io_jmap::{
    rfc8620::{
        blob_upload::{JmapBlobUpload, JmapBlobUploadError, JmapBlobUploadResult},
        session::JmapSession,
    },
    rfc8621::{
        capabilities,
        email::EmailImport,
        email_import::{JmapEmailImport, JmapEmailImportError, JmapEmailImportResult},
        mailbox_query::{JmapMailboxQuery, JmapMailboxQueryError, JmapMailboxQueryResult},
    },
};
use log::trace;
use secrecy::SecretString;
use thiserror::Error;
use url::Url;

use crate::jmap::message_copy::find_mailbox_id;

/// Errors produced while orchestrating Blob/upload + Mailbox/query +
/// Email/import.
#[derive(Debug, Error)]
pub enum JmapMessageAddError {
    #[error(transparent)]
    BlobUpload(#[from] JmapBlobUploadError),
    #[error(transparent)]
    MailboxQuery(#[from] JmapMailboxQueryError),
    #[error(transparent)]
    EmailImport(#[from] JmapEmailImportError),
    #[error("no JMAP mailbox matched the name {0:?}")]
    UnknownMailbox(String),
    #[error("Email/import did not create the imported email")]
    ImportFailed,
    #[error("Resolved JMAP upload URL is invalid: {0}")]
    InvalidUploadUrl(String),
    #[error("JMAP message add was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`JmapMessageAdd::resume`].
#[derive(Debug)]
pub enum JmapMessageAddResult {
    Ok(Option<String>),
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapMessageAddError),
}

enum State {
    Uploading {
        upload: JmapBlobUpload,
        session: JmapSession,
        http_auth: SecretString,
        mailbox_name: String,
        keywords: Vec<String>,
    },
    Resolving {
        query: JmapMailboxQuery,
        session: JmapSession,
        http_auth: SecretString,
        mailbox_name: String,
        keywords: Vec<String>,
        blob_id: String,
    },
    Importing(JmapEmailImport),
    Done,
}

/// I/O-free orchestrator: Blob/upload → Mailbox/query (resolve name) →
/// Email/import.
pub struct JmapMessageAdd {
    state: State,
}

impl JmapMessageAdd {
    pub fn new(
        session: &JmapSession,
        http_auth: &SecretString,
        raw: Vec<u8>,
        mailbox_name: impl ToString,
        keywords: impl IntoIterator<Item = String>,
    ) -> Result<Self, JmapMessageAddError> {
        trace!("prepare JMAP message add");

        let account_id = session
            .primary_accounts
            .get(capabilities::MAIL)
            .cloned()
            .unwrap_or_default();

        let upload_url_str = session.upload_url.replace("{accountId}", &account_id);
        let upload_url = Url::parse(&upload_url_str)
            .map_err(|_| JmapMessageAddError::InvalidUploadUrl(upload_url_str))?;

        let upload = JmapBlobUpload::new(http_auth, &upload_url, "message/rfc822", raw);

        Ok(Self {
            state: State::Uploading {
                upload,
                session: session.clone(),
                http_auth: http_auth.clone(),
                mailbox_name: mailbox_name.to_string(),
                keywords: keywords.into_iter().collect(),
            },
        })
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> JmapMessageAddResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Uploading {
                    mut upload,
                    session,
                    http_auth,
                    mailbox_name,
                    keywords,
                } => match upload.resume(arg.take()) {
                    JmapBlobUploadResult::WantsRead => {
                        self.state = State::Uploading {
                            upload,
                            session,
                            http_auth,
                            mailbox_name,
                            keywords,
                        };
                        return JmapMessageAddResult::WantsRead;
                    }
                    JmapBlobUploadResult::WantsWrite(bytes) => {
                        self.state = State::Uploading {
                            upload,
                            session,
                            http_auth,
                            mailbox_name,
                            keywords,
                        };
                        return JmapMessageAddResult::WantsWrite(bytes);
                    }
                    JmapBlobUploadResult::Err(err) => return JmapMessageAddResult::Err(err.into()),
                    JmapBlobUploadResult::Ok { blob_id, .. } => {
                        let query = match JmapMailboxQuery::new(
                            &session, &http_auth, None, None, None, None, None,
                        ) {
                            Ok(q) => q,
                            Err(err) => return JmapMessageAddResult::Err(err.into()),
                        };

                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            mailbox_name,
                            keywords,
                            blob_id,
                        };
                    }
                },
                State::Resolving {
                    mut query,
                    session,
                    http_auth,
                    mailbox_name,
                    keywords,
                    blob_id,
                } => match query.resume(arg.take()) {
                    JmapMailboxQueryResult::WantsRead => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            mailbox_name,
                            keywords,
                            blob_id,
                        };
                        return JmapMessageAddResult::WantsRead;
                    }
                    JmapMailboxQueryResult::WantsWrite(bytes) => {
                        self.state = State::Resolving {
                            query,
                            session,
                            http_auth,
                            mailbox_name,
                            keywords,
                            blob_id,
                        };
                        return JmapMessageAddResult::WantsWrite(bytes);
                    }
                    JmapMailboxQueryResult::Err(err) => {
                        return JmapMessageAddResult::Err(err.into());
                    }
                    JmapMailboxQueryResult::Ok { mailboxes, .. } => {
                        let Some(mailbox_id) = find_mailbox_id(&mailboxes, &mailbox_name) else {
                            return JmapMessageAddResult::Err(JmapMessageAddError::UnknownMailbox(
                                mailbox_name,
                            ));
                        };

                        let mut mailbox_ids = BTreeMap::new();
                        mailbox_ids.insert(mailbox_id, true);

                        let kw = if keywords.is_empty() {
                            None
                        } else {
                            Some(keywords.into_iter().map(|k| (k, true)).collect())
                        };

                        let mut emails = BTreeMap::new();
                        emails.insert(
                            "added".into(),
                            EmailImport {
                                blob_id,
                                mailbox_ids,
                                keywords: kw,
                                received_at: None,
                            },
                        );

                        let import = match JmapEmailImport::new(&session, &http_auth, emails) {
                            Ok(c) => c,
                            Err(err) => return JmapMessageAddResult::Err(err.into()),
                        };

                        self.state = State::Importing(import);
                    }
                },
                State::Importing(mut import) => match import.resume(arg.take()) {
                    JmapEmailImportResult::WantsRead => {
                        self.state = State::Importing(import);
                        return JmapMessageAddResult::WantsRead;
                    }
                    JmapEmailImportResult::WantsWrite(bytes) => {
                        self.state = State::Importing(import);
                        return JmapMessageAddResult::WantsWrite(bytes);
                    }
                    JmapEmailImportResult::Err(err) => {
                        return JmapMessageAddResult::Err(err.into());
                    }
                    JmapEmailImportResult::Ok {
                        mut created,
                        not_created,
                        ..
                    } => {
                        if !not_created.is_empty() || created.is_empty() {
                            return JmapMessageAddResult::Err(JmapMessageAddError::ImportFailed);
                        }

                        let email_id = created.remove("added").and_then(|e| e.id);
                        return JmapMessageAddResult::Ok(email_id);
                    }
                },
                State::Done => return JmapMessageAddResult::Err(JmapMessageAddError::AlreadyDone),
            }
        }
    }
}
