//! Decides what a peer may ask for, before any handler runs.

use crate::model::{Peer, Permissions};
use crate::protocol::Request;

pub const UNPAIRED: &str = "These computers aren't paired. Pair them, then try again.";
pub const IDENTITY_CHANGED: &str = "This computer's identity changed. Unpair it and pair again.";
/// The same news, for the computer that noticed it.
pub const IDENTITY_CHANGED_THERE: &str = "its identity changed since you paired. If Project \
     Transfer was reinstalled on it, unpair it and pair again.";

/// Why a request was turned away. "This computer" is the asker in `theirs`
/// and the refusing computer in `ours`, so each side reads it as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Sent back to the computer that asked.
    pub theirs: String,
    /// Shown here, after "Turned away <name>: ".
    pub ours: String,
}

impl Refusal {
    /// For reasons that read the same from either side.
    pub fn same(text: impl Into<String>) -> Refusal {
        let text = text.into();
        Refusal {
            theirs: text.clone(),
            ours: text,
        }
    }

    pub fn identity_changed() -> Refusal {
        Refusal {
            theirs: IDENTITY_CHANGED.into(),
            ours: IDENTITY_CHANGED_THERE.into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Need {
    /// Hello and pairing: allowed for anyone who completed the handshake.
    Nothing,
    Paired,
    PullOrPush,
    Pull,
    Push,
}

pub fn need(req: &Request) -> Need {
    use Request as R;
    match req {
        R::Hello { .. } | R::PairCommit { .. } | R::PairReveal { .. } | R::PairFinal { .. } => {
            Need::Nothing
        }
        // The relay itself checks that it is paired with the target too.
        R::Status | R::Relay { .. } => Need::Paired,
        // A push preview reads the receiver's project and manifest, and
        // commands are exchanged on both push and pull.
        // Hashes too: a push preview hashes the receiver's copies.
        R::ProjectInfo { .. }
        | R::Manifest { .. }
        | R::Hashes { .. }
        | R::ExchangeCommands { .. } => Need::PullOrPush,
        R::GetFile { .. } | R::EndPull { .. } => Need::Pull,
        R::BeginPush { .. }
        | R::PutFile { .. }
        | R::MakeDir { .. }
        | R::SetMtime { .. }
        | R::MakeSymlink { .. }
        | R::Remove { .. }
        | R::EndPush
        // Both decide where pushed files land.
        | R::SyncProject { .. }
        | R::SetFolderPath { .. } => Need::Push,
    }
}

/// `peer` is the stored peer matching both the Hello id and the certificate.
/// `me` is this instance's name, for the refusal text the peer will show.
pub fn check(req: &Request, peer: Option<&Peer>, me: &str) -> Result<(), Refusal> {
    let n = need(req);
    if n == Need::Nothing {
        return Ok(());
    }
    let Some(peer) = peer else {
        return Err(Refusal {
            theirs: UNPAIRED.into(),
            ours: "it isn't paired with this computer.".into(),
        });
    };
    if permits(&n, peer.allows) {
        return Ok(());
    }
    let (theirs, tried) = match n {
        Need::Pull => (
            format!(
                "{me} doesn't let this computer pull from it. Allow pulling for it on {me}, then try again."
            ),
            "it tried to pull from this computer, which you haven't allowed.",
        ),
        Need::Push => (
            format!(
                "{me} doesn't let this computer push to it. Allow pushing for it on {me}, then try again."
            ),
            "it tried to push to this computer, which you haven't allowed.",
        ),
        _ => (
            format!(
                "{me} doesn't let this computer push to it or pull from it. Change its permissions on {me}, then try again."
            ),
            "it asked about a project, but you haven't allowed it to push to or pull from this \
             computer.",
        ),
    };
    Err(Refusal {
        theirs,
        ours: format!("{tried} Allow it under Manage paired computers if you meant to."),
    })
}

/// Whether a paired computer that grants `granted` answers `req`. The asking
/// side checks this first: every refusal shows as a warning on the other
/// computer, so asking for what it won't share fills its activity.
pub fn may_ask(req: &Request, granted: Permissions) -> bool {
    permits(&need(req), granted)
}

fn permits(n: &Need, a: Permissions) -> bool {
    match n {
        Need::Nothing | Need::Paired => true,
        Need::PullOrPush => a.may_pull_from_me || a.may_push_to_me,
        Need::Pull => a.may_pull_from_me,
        Need::Push => a.may_push_to_me,
    }
}

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
