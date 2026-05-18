//! JMAP flag add (`Email/set` with `keywords/<name>: true`), wrapping
//! [`io_jmap::rfc8621::email_set::JmapEmailSet`].

use alloc::{string::String, vec::Vec};

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::email_set::{JmapEmailSet, JmapEmailSetArgs, JmapEmailSetError, JmapEmailSetResult},
};
use log::trace;
use secrecy::SecretString;

/// Result returned by [`JmapFlagAdd::resume`].
#[derive(Debug)]
pub enum JmapFlagAddResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapEmailSetError),
}

/// I/O-free coroutine adding keywords to a set of emails.
pub struct JmapFlagAdd {
    inner: JmapEmailSet,
}

impl JmapFlagAdd {
    pub fn new<I, J>(
        session: &JmapSession,
        http_auth: &SecretString,
        ids: I,
        keywords: J,
    ) -> Result<Self, JmapEmailSetError>
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String> + Clone,
    {
        trace!("prepare JMAP flag add");

        let mut args = JmapEmailSetArgs::default();
        for id in ids {
            for keyword in keywords.clone() {
                args.set_keyword(id.clone(), keyword);
            }
        }

        let inner = JmapEmailSet::new(session, http_auth, args)?;
        Ok(Self { inner })
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> JmapFlagAddResult {
        match self.inner.resume(arg) {
            JmapEmailSetResult::WantsRead => JmapFlagAddResult::WantsRead,
            JmapEmailSetResult::WantsWrite(bytes) => JmapFlagAddResult::WantsWrite(bytes),
            JmapEmailSetResult::Ok { .. } => JmapFlagAddResult::Ok,
            JmapEmailSetResult::Err(err) => JmapFlagAddResult::Err(err),
        }
    }
}
