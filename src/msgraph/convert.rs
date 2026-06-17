//! Shared helpers for the Microsoft Graph backend: mail folder and
//! message conversion to the shared [`Mailbox`] / [`Envelope`] shapes,
//! and flag translation to a message PATCH body.
//!
//! Graph models flags as heterogeneous scalar fields rather than a flag
//! set: `\Seen` is `isRead`, `\Flagged` is the follow-up flag,
//! `$Important` is `importance = high`, `\Draft` is the read-only
//! `isDraft`, and custom keywords are `categories`. Flags without a
//! Graph equivalent (Answered, Forwarded, Junk, …) are dropped.

use alloc::{collections::BTreeSet, string::ToString};

use chrono::{DateTime, FixedOffset};
use io_msgraph::v1::rest::users::{
    mail_folders::MsgraphMailFolder,
    messages::{
        MsgraphFlagStatus, MsgraphFollowupFlag, MsgraphImportance, MsgraphMessage, MsgraphRecipient,
    },
};

use crate::{
    address::Address,
    envelope::types::{Envelope, normalize_message_id},
    flag::types::{Flag, FlagOp, IanaFlag},
    mailbox::types::Mailbox,
};

/// OData `$select` for envelope listing: the message fields backing the
/// shared [`Envelope`], so Graph returns them in one round-trip.
pub(crate) const ENVELOPE_SELECT: &str = "id,subject,from,toRecipients,receivedDateTime,isRead,isDraft,hasAttachments,internetMessageId,importance,flag,categories";

/// Converts one Graph mail folder into the shared [`Mailbox`] shape;
/// Graph folders carry their counts inline, so no extra fetch is needed.
pub(crate) fn mailbox_from(folder: MsgraphMailFolder) -> Mailbox {
    Mailbox {
        id: folder.id,
        name: folder.display_name,
        total: folder.total_item_count,
        unread: folder.unread_item_count,
    }
}

/// Folds a Graph [`MsgraphMessage`] into the shared [`Envelope`] shape.
///
/// `size` is left `0`: the message resource does not expose a raw size
/// in the listing selection.
pub(crate) fn envelope_from(message: MsgraphMessage) -> Envelope {
    let flags = flags_from_message(&message);
    let message_id = message
        .internet_message_id
        .as_deref()
        .and_then(normalize_message_id);
    let date = message
        .received_date_time
        .as_deref()
        .and_then(parse_rfc3339);
    let from = message.from.map(address_from).into_iter().collect();
    let to = message
        .to_recipients
        .into_iter()
        .map(address_from)
        .collect();

    Envelope {
        id: message.id,
        message_id,
        flags,
        subject: message.subject.unwrap_or_default(),
        from,
        to,
        date,
        size: 0,
        has_attachment: message.has_attachments,
    }
}

/// Builds the message PATCH body applying `flags` under `op`.
///
/// `Set` drives every scalar flag-field to its target and replaces
/// `categories` with the custom keywords; `Add` / `Remove` only touch
/// the scalar fields the operation mentions (custom-keyword add/remove
/// would need a read-modify-write and is left to a later iteration). An
/// empty (default) patch means there is nothing to send.
pub(crate) fn flag_patch(flags: &[Flag], op: FlagOp) -> MsgraphMessage {
    let mut patch = MsgraphMessage::default();

    match op {
        FlagOp::Set => {
            patch.is_read = Some(flags.iter().any(Flag::is_seen));
            patch.flag = Some(followup(flags.iter().any(Flag::is_flagged)));
            patch.importance = Some(importance(flags.iter().any(Flag::is_important)));
            patch.categories = flags
                .iter()
                .filter(|flag| flag.iana().is_none())
                .map(|flag| flag.raw().to_string())
                .collect();
        }
        FlagOp::Add => {
            for flag in flags {
                if flag.is_seen() {
                    patch.is_read = Some(true);
                }
                if flag.is_flagged() {
                    patch.flag = Some(followup(true));
                }
                if flag.is_important() {
                    patch.importance = Some(MsgraphImportance::High);
                }
            }
        }
        FlagOp::Remove => {
            for flag in flags {
                if flag.is_seen() {
                    patch.is_read = Some(false);
                }
                if flag.is_flagged() {
                    patch.flag = Some(followup(false));
                }
                if flag.is_important() {
                    patch.importance = Some(MsgraphImportance::Normal);
                }
            }
        }
    }

    patch
}

/// Derives the shared flag set from a Graph message's scalar fields and
/// categories.
fn flags_from_message(message: &MsgraphMessage) -> BTreeSet<Flag> {
    let mut flags = BTreeSet::new();

    if message.is_read == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Seen));
    }
    if matches!(
        message.flag.as_ref().and_then(|flag| flag.flag_status),
        Some(MsgraphFlagStatus::Flagged)
    ) {
        flags.insert(Flag::from_iana(IanaFlag::Flagged));
    }
    if message.importance == Some(MsgraphImportance::High) {
        flags.insert(Flag::from_iana(IanaFlag::Important));
    }
    if message.is_draft == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Draft));
    }
    for category in &message.categories {
        flags.insert(Flag::from_raw(category.clone()));
    }

    flags
}

/// Converts a Graph recipient into a shared [`Address`].
fn address_from(recipient: MsgraphRecipient) -> Address {
    Address {
        name: recipient.email_address.name,
        email: recipient.email_address.address.unwrap_or_default(),
    }
}

fn followup(flagged: bool) -> MsgraphFollowupFlag {
    let flag_status = if flagged {
        MsgraphFlagStatus::Flagged
    } else {
        MsgraphFlagStatus::NotFlagged
    };
    MsgraphFollowupFlag {
        flag_status: Some(flag_status),
    }
}

fn importance(important: bool) -> MsgraphImportance {
    if important {
        MsgraphImportance::High
    } else {
        MsgraphImportance::Normal
    }
}

fn parse_rfc3339(raw: &str) -> Option<DateTime<FixedOffset>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(trimmed).ok()
}
