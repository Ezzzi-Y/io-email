//! JMAP flag delete (`Email/set` with `keywords/<name>: null`), wrapping
//! [`io_jmap::rfc8621::email_set::JmapEmailSet`].

use alloc::{string::String, vec::Vec};

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::email_set::{JmapEmailSet, JmapEmailSetArgs, JmapEmailSetError, JmapEmailSetResult},
};
use log::trace;
use secrecy::SecretString;

/// Result returned by [`JmapFlagDelete::resume`].
#[derive(Debug)]
pub enum JmapFlagDeleteResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapEmailSetError),
}

/// I/O-free coroutine removing keywords from a set of emails.
pub struct JmapFlagDelete {
    inner: JmapEmailSet,
}

impl JmapFlagDelete {
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
        trace!("prepare JMAP flag delete");

        let mut args = JmapEmailSetArgs::default();
        for id in ids {
            for keyword in keywords.clone() {
                args.unset_keyword(id.clone(), keyword);
            }
        }

        let inner = JmapEmailSet::new(session, http_auth, args)?;
        Ok(Self { inner })
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> JmapFlagDeleteResult {
        match self.inner.resume(arg) {
            JmapEmailSetResult::WantsRead => JmapFlagDeleteResult::WantsRead,
            JmapEmailSetResult::WantsWrite(bytes) => JmapFlagDeleteResult::WantsWrite(bytes),
            JmapEmailSetResult::Ok { .. } => JmapFlagDeleteResult::Ok,
            JmapEmailSetResult::Err(err) => JmapFlagDeleteResult::Err(err),
        }
    }
}
