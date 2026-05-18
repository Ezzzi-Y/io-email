//! JMAP flag set (`Email/set` with replace-keywords), wrapping
//! [`io_jmap::rfc8621::email_set::JmapEmailSet`].

use alloc::{collections::BTreeMap, string::String, vec::Vec};

use io_jmap::{
    rfc8620::session::JmapSession,
    rfc8621::email_set::{JmapEmailSet, JmapEmailSetArgs, JmapEmailSetError, JmapEmailSetResult},
};
use log::trace;
use secrecy::SecretString;

/// Result returned by [`JmapFlagSet::resume`].
#[derive(Debug)]
pub enum JmapFlagSetResult {
    Ok,
    WantsRead,
    WantsWrite(Vec<u8>),
    Err(JmapEmailSetError),
}

/// I/O-free coroutine replacing the keyword set on a list of emails.
/// Any prior keyword absent from the new set is removed.
pub struct JmapFlagSet {
    inner: JmapEmailSet,
}

impl JmapFlagSet {
    pub fn new<I, J>(
        session: &JmapSession,
        http_auth: &SecretString,
        ids: I,
        keywords: J,
    ) -> Result<Self, JmapEmailSetError>
    where
        I: IntoIterator<Item = String>,
        J: IntoIterator<Item = String>,
    {
        trace!("prepare JMAP flag set");

        let mut keywords_map = BTreeMap::new();
        for keyword in keywords {
            keywords_map.insert(keyword, true);
        }

        let mut args = JmapEmailSetArgs::default();
        for id in ids {
            args.replace_keywords(id, keywords_map.clone());
        }

        let inner = JmapEmailSet::new(session, http_auth, args)?;
        Ok(Self { inner })
    }

    pub fn resume(&mut self, arg: Option<&[u8]>) -> JmapFlagSetResult {
        match self.inner.resume(arg) {
            JmapEmailSetResult::WantsRead => JmapFlagSetResult::WantsRead,
            JmapEmailSetResult::WantsWrite(bytes) => JmapFlagSetResult::WantsWrite(bytes),
            JmapEmailSetResult::Ok { .. } => JmapFlagSetResult::Ok,
            JmapEmailSetResult::Err(err) => JmapFlagSetResult::Err(err),
        }
    }
}
