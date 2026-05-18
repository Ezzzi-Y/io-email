//! JMAP message send (`Blob/upload` + `Email/import` +
//! `EmailSubmission/set`), wrapping a private orchestrator that
//! uploads raw bytes, imports them into the caller-provided drafts
//! mailbox with the `$draft` keyword, then submits them with the
//! caller-provided identity.

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
        email_submission::EmailSubmissionCreate,
        email_submission_set::{
            JmapEmailSubmissionSet, JmapEmailSubmissionSetError, JmapEmailSubmissionSetResult,
        },
    },
};
use log::trace;
use secrecy::SecretString;
use thiserror::Error;
use url::Url;

/// Errors produced while orchestrating Blob/upload + Email/import +
/// EmailSubmission/set for JMAP message submission.
#[derive(Debug, Error)]
pub enum JmapMessageSendError {
    #[error(transparent)]
    BlobUpload(#[from] JmapBlobUploadError),
    #[error(transparent)]
    EmailImport(#[from] JmapEmailImportError),
    #[error(transparent)]
    EmailSubmission(#[from] JmapEmailSubmissionSetError),
    #[error("Email/import did not create the staged email")]
    ImportFailed,
    #[error("Email/import response did not include an email id")]
    MissingImportedEmailId,
    #[error("EmailSubmission/set did not submit the email")]
    SubmissionFailed,
    #[error("Resolved JMAP upload URL is invalid: {0}")]
    InvalidUploadUrl(String),
    #[error("JMAP message send was resumed after completion")]
    AlreadyDone,
}

/// Result returned by [`JmapMessageSend::resume`].
#[derive(Debug)]
pub enum JmapMessageSendResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapMessageSendError),
}

enum State {
    Uploading {
        upload: JmapBlobUpload,
        session: JmapSession,
        http_auth: SecretString,
        drafts_mailbox_id: String,
        identity_id: String,
    },
    Importing {
        import: JmapEmailImport,
        session: JmapSession,
        http_auth: SecretString,
        identity_id: String,
    },
    Submitting(JmapEmailSubmissionSet),
    Done,
}

/// I/O-free orchestrator: `Blob/upload` → `Email/import` →
/// `EmailSubmission/set`.
///
/// The caller supplies the identity to send as and the drafts mailbox
/// id to stage the email in. Both come from `Identity/get` and
/// `Mailbox/query` (role = `drafts`) at session startup.
pub struct JmapMessageSend {
    state: State,
}

impl JmapMessageSend {
    pub fn new(
        session: &JmapSession,
        http_auth: &SecretString,
        raw: Vec<u8>,
        identity_id: impl ToString,
        drafts_mailbox_id: impl ToString,
    ) -> Result<Self, JmapMessageSendError> {
        trace!("prepare JMAP message send");

        let account_id = session
            .primary_accounts
            .get(capabilities::MAIL)
            .cloned()
            .unwrap_or_default();

        let upload_url_str = resolve_upload_url(&session.upload_url, &account_id);
        let upload_url = Url::parse(&upload_url_str)
            .map_err(|_| JmapMessageSendError::InvalidUploadUrl(upload_url_str))?;

        let upload = JmapBlobUpload::new(http_auth, &upload_url, "message/rfc822", raw);

        Ok(Self {
            state: State::Uploading {
                upload,
                session: session.clone(),
                http_auth: http_auth.clone(),
                drafts_mailbox_id: drafts_mailbox_id.to_string(),
                identity_id: identity_id.to_string(),
            },
        })
    }

    pub fn resume(&mut self, mut arg: Option<&[u8]>) -> JmapMessageSendResult {
        loop {
            match mem::replace(&mut self.state, State::Done) {
                State::Uploading {
                    mut upload,
                    session,
                    http_auth,
                    drafts_mailbox_id,
                    identity_id,
                } => match upload.resume(arg.take()) {
                    JmapBlobUploadResult::WantsRead => {
                        self.state = State::Uploading {
                            upload,
                            session,
                            http_auth,
                            drafts_mailbox_id,
                            identity_id,
                        };
                        return JmapMessageSendResult::WantsRead;
                    }
                    JmapBlobUploadResult::WantsWrite(bytes) => {
                        self.state = State::Uploading {
                            upload,
                            session,
                            http_auth,
                            drafts_mailbox_id,
                            identity_id,
                        };
                        return JmapMessageSendResult::WantsWrite(bytes);
                    }
                    JmapBlobUploadResult::Err(err) => {
                        return JmapMessageSendResult::Err(err.into());
                    }
                    JmapBlobUploadResult::Ok { blob_id, .. } => {
                        let mut mailbox_ids = BTreeMap::new();
                        mailbox_ids.insert(drafts_mailbox_id, true);

                        let mut keywords = BTreeMap::new();
                        keywords.insert("$draft".into(), true);

                        let mut emails = BTreeMap::new();
                        emails.insert(
                            "outgoing".into(),
                            EmailImport {
                                blob_id,
                                mailbox_ids,
                                keywords: Some(keywords),
                                received_at: None,
                            },
                        );

                        let import = match JmapEmailImport::new(&session, &http_auth, emails) {
                            Ok(c) => c,
                            Err(err) => return JmapMessageSendResult::Err(err.into()),
                        };

                        self.state = State::Importing {
                            import,
                            session,
                            http_auth,
                            identity_id,
                        };
                    }
                },
                State::Importing {
                    mut import,
                    session,
                    http_auth,
                    identity_id,
                } => match import.resume(arg.take()) {
                    JmapEmailImportResult::WantsRead => {
                        self.state = State::Importing {
                            import,
                            session,
                            http_auth,
                            identity_id,
                        };
                        return JmapMessageSendResult::WantsRead;
                    }
                    JmapEmailImportResult::WantsWrite(bytes) => {
                        self.state = State::Importing {
                            import,
                            session,
                            http_auth,
                            identity_id,
                        };
                        return JmapMessageSendResult::WantsWrite(bytes);
                    }
                    JmapEmailImportResult::Err(err) => {
                        return JmapMessageSendResult::Err(err.into());
                    }
                    JmapEmailImportResult::Ok {
                        mut created,
                        not_created,
                        ..
                    } => {
                        if !not_created.is_empty() {
                            return JmapMessageSendResult::Err(JmapMessageSendError::ImportFailed);
                        }

                        let Some(email) = created.remove("outgoing") else {
                            return JmapMessageSendResult::Err(JmapMessageSendError::ImportFailed);
                        };

                        let Some(email_id) = email.id else {
                            return JmapMessageSendResult::Err(
                                JmapMessageSendError::MissingImportedEmailId,
                            );
                        };

                        let mut submissions = BTreeMap::new();
                        submissions.insert(
                            "outgoing".into(),
                            EmailSubmissionCreate {
                                identity_id,
                                email_id,
                                envelope: None,
                            },
                        );

                        let submit =
                            match JmapEmailSubmissionSet::new(&session, &http_auth, submissions) {
                                Ok(c) => c,
                                Err(err) => return JmapMessageSendResult::Err(err.into()),
                            };

                        self.state = State::Submitting(submit);
                    }
                },
                State::Submitting(mut submit) => match submit.resume(arg.take()) {
                    JmapEmailSubmissionSetResult::WantsRead => {
                        self.state = State::Submitting(submit);
                        return JmapMessageSendResult::WantsRead;
                    }
                    JmapEmailSubmissionSetResult::WantsWrite(bytes) => {
                        self.state = State::Submitting(submit);
                        return JmapMessageSendResult::WantsWrite(bytes);
                    }
                    JmapEmailSubmissionSetResult::Err(err) => {
                        return JmapMessageSendResult::Err(err.into());
                    }
                    JmapEmailSubmissionSetResult::Ok { not_created, .. } => {
                        if !not_created.is_empty() {
                            return JmapMessageSendResult::Err(
                                JmapMessageSendError::SubmissionFailed,
                            );
                        }

                        return JmapMessageSendResult::Ok;
                    }
                },
                State::Done => {
                    return JmapMessageSendResult::Err(JmapMessageSendError::AlreadyDone);
                }
            }
        }
    }
}

fn resolve_upload_url(template: &str, account_id: &str) -> String {
    template.replace("{accountId}", account_id)
}
