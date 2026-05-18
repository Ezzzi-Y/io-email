//! SMTP message send, wrapping [`io_smtp::send::SmtpMessageSend`].

use alloc::vec::Vec;

use io_smtp::{
    rfc5321::types::{forward_path::ForwardPath, reverse_path::ReversePath},
    send::{
        SmtpMessageSend as InnerSmtpMessageSend, SmtpMessageSendError,
        SmtpMessageSendResult as InnerSmtpMessageSendResult,
    },
};
use log::trace;

/// Result returned by [`SmtpMessageSend::resume`].
#[derive(Debug)]
pub enum SmtpMessageSendResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(SmtpMessageSendError),
}

/// I/O-free coroutine running the RFC 5321 mail transaction (MAIL FROM,
/// RCPT TO, DATA).
pub struct SmtpMessageSend {
    inner: InnerSmtpMessageSend,
}

impl SmtpMessageSend {
    pub fn new<'a>(
        reverse_path: ReversePath,
        forward_paths: impl IntoIterator<Item = ForwardPath<'a>>,
        message: Vec<u8>,
    ) -> Self {
        trace!("prepare SMTP message send");
        Self {
            inner: InnerSmtpMessageSend::new(reverse_path, forward_paths, message),
        }
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> SmtpMessageSendResult {
        match self.inner.resume(arg) {
            InnerSmtpMessageSendResult::Ok => SmtpMessageSendResult::Ok,
            InnerSmtpMessageSendResult::WantsRead => SmtpMessageSendResult::WantsRead,
            InnerSmtpMessageSendResult::WantsWrite(bytes) => {
                SmtpMessageSendResult::WantsWrite(bytes)
            }
            InnerSmtpMessageSendResult::Err(err) => SmtpMessageSendResult::Err(err),
        }
    }
}
