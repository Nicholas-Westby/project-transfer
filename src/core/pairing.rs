//! Pairing in both directions, and what the server reports.

use super::reach::open_any;
use super::state::{OutgoingPairView, PairPromptView, PairState, PromptState};
use super::{Core, lock};
use crate::discovery::Discovered;
use crate::model::Permissions;
use crate::net::{NetEvent, pair_finish, pair_start};
use std::sync::Arc;
use tokio::sync::{Notify, oneshot};
use tokio::task::AbortHandle;

/// A pairing another computer started, while this one can still answer it.
pub(super) struct Incoming {
    reply: Option<oneshot::Sender<Option<Permissions>>>,
    cancel: Arc<Notify>,
}

/// A pairing this computer started.
pub(super) struct Outgoing {
    confirm: Option<oneshot::Sender<bool>>,
    task: Option<AbortHandle>,
}

impl Core {
    /// Starts pairing in the background; a pairing already running here is
    /// stopped first, which closes its connection.
    pub(super) fn start_pair(
        &self,
        target: Discovered,
        requested: Permissions,
        offered: Permissions,
    ) {
        let (confirm, confirmed) = oneshot::channel();
        if let Some(old) = lock(&self.outgoing).replace(Outgoing {
            confirm: Some(confirm),
            task: None,
        }) && let Some(t) = old.task
        {
            t.abort();
        }
        let task = tokio::spawn(self.clone().pair(target, requested, offered, confirmed));
        if let Some(o) = lock(&self.outgoing).as_mut() {
            o.task = Some(task.abort_handle());
        }
    }

    async fn pair(
        self,
        target: Discovered,
        requested: Permissions,
        offered: Permissions,
        confirmed: oneshot::Receiver<bool>,
    ) {
        let name = target.name.clone();
        self.ui.update(|s| {
            s.pairing = Some(OutgoingPairView {
                target_name: name.clone(),
                code: None,
                state: PairState::Connecting,
                other_accepted: false,
            })
        });
        let result = async {
            let mut conn = open_any(&self, &target).await?;
            let started = pair_start(&mut conn, &self.shared, requested, offered).await?;
            let code = started.code.clone();
            self.ui.update(|s| {
                if let Some(p) = &mut s.pairing {
                    p.code = Some(code.clone());
                    p.state = PairState::Confirm;
                }
            });
            self.ui.info(format!(
                "Asked {name} to pair. Check that it shows the code {code}."
            ));
            let confirm = async { confirmed.await.unwrap_or(false) };
            let ui = self.ui.clone();
            let on_accepted = move || {
                ui.update(|s| {
                    if let Some(p) = &mut s.pairing {
                        p.other_accepted = true;
                    }
                })
            };
            pair_finish(&mut conn, &self.shared, started, confirm, on_accepted).await
        }
        .await;
        let state = match result {
            // NetEvent::Paired records it in the activity strip.
            Ok(_) => PairState::Done,
            Err(e) => {
                let why = format!("{e:#}");
                self.ui.warn(format!("Did not pair with {name}: {why}"));
                PairState::Failed(why)
            }
        };
        self.ui.update(|s| {
            if let Some(p) = &mut s.pairing {
                p.state = state;
            }
        });
    }

    /// This computer's user says whether the code on both screens matches.
    pub(super) fn confirm_code(&self, matches: bool) {
        let sender = lock(&self.outgoing).as_mut().and_then(|o| o.confirm.take());
        if let Some(tx) = sender {
            let _ = tx.send(matches);
        }
        if matches {
            self.ui.update(|s| {
                if let Some(p) = &mut s.pairing
                    && p.state == PairState::Confirm
                {
                    p.state = PairState::Waiting;
                }
            });
        }
    }

    /// Closes the outgoing pairing view, stopping the pairing if it is still
    /// running so the other computer's prompt goes away and nothing is stored.
    pub(super) fn dismiss_pairing(&self) {
        if let Some(o) = lock(&self.outgoing).take()
            && let Some(t) = o.task
        {
            t.abort();
        }
        self.ui.update(|s| s.pairing = None);
    }

    pub(super) fn answer_pair(&self, answer: Option<Permissions>) {
        let reply = lock(&self.incoming).as_mut().and_then(|i| i.reply.take());
        let name = self
            .ui
            .lock()
            .pair_prompt
            .as_ref()
            .map_or_else(|| "the other computer".to_string(), |p| p.from_name.clone());
        let sent = reply.is_some_and(|r| r.send(answer).is_ok());
        if !sent {
            lock(&self.incoming).take();
            self.ui.update(|s| s.pair_prompt = None);
            self.ui.warn(format!(
                "The pairing request from {name} expired. Start pairing again on that computer."
            ));
        } else if answer.is_none() {
            lock(&self.incoming).take();
            self.ui.update(|s| s.pair_prompt = None);
            self.ui.info(format!("Declined pairing with {name}."));
        } else {
            self.ui.update(|s| {
                if let Some(p) = &mut s.pair_prompt {
                    p.state = PromptState::Waiting;
                }
            });
            self.ui.info(format!(
                "Accepted pairing with {name}. Waiting for {name} to confirm the code."
            ));
        }
    }

    /// Closes the incoming pairing view. While the other computer has not
    /// confirmed yet, this withdraws the acceptance so nothing is stored.
    pub(super) fn dismiss_prompt(&self) {
        let prompt = self.ui.update(|s| s.pair_prompt.take());
        let incoming = lock(&self.incoming).take();
        if let (Some(p), Some(i)) = (prompt, incoming)
            && p.state == PromptState::Waiting
        {
            i.cancel.notify_one();
            self.ui.info(format!(
                "Cancelled pairing with {}. Nothing was saved.",
                p.from_name
            ));
        }
    }

    pub(super) async fn net_event(&self, ev: NetEvent) {
        match ev {
            NetEvent::PairPrompt {
                from_id,
                from_name,
                code,
                requested,
                offered,
                reply,
                cancel,
            } => {
                // The server allows one prompt at a time; a stale sender is
                // dropped, which declines it.
                *lock(&self.incoming) = Some(Incoming {
                    reply: Some(reply),
                    cancel,
                });
                self.ui.info(format!(
                    "{from_name} asks to pair. Check that it shows the code {code}."
                ));
                self.ui.update(|s| {
                    s.pair_prompt = Some(PairPromptView {
                        from_id,
                        from_name,
                        code,
                        requested,
                        offered,
                        state: PromptState::Asking,
                    })
                });
            }
            NetEvent::PairEnded { from_id, reason } => {
                lock(&self.incoming).take();
                self.ui.warn(reason.clone());
                self.ui.update(|s| {
                    if let Some(p) = &mut s.pair_prompt
                        && p.from_id == from_id
                    {
                        p.state = PromptState::Failed(reason);
                    }
                });
            }
            NetEvent::Paired(peer) => {
                self.ui.update(|s| {
                    if let Some(p) = &mut s.pair_prompt
                        && p.from_id == peer.id
                    {
                        p.state = PromptState::Done;
                    }
                });
                lock(&self.incoming).take();
                self.sync_peers().await;
                self.ui.info(format!("Paired with {}.", peer.name));
                if self.ui.lock().selected_peer.is_none()
                    && let Err(e) = self.select_peer(peer.id).await
                {
                    self.ui.error(format!("{e:#}"));
                }
            }
            NetEvent::Refused { peer_name, reason } => {
                self.ui.warn(format!("Turned away {peer_name}: {reason}"));
            }
            NetEvent::Received {
                project,
                peer,
                files,
                failed,
            } => {
                self.sync_projects().await;
                let (what, who) = {
                    let s = self.ui.lock();
                    (
                        s.project(project)
                            .map_or("a project".into(), |p| p.name.clone()),
                        s.peer(peer)
                            .map_or("a paired computer".into(), |v| v.peer.name.clone()),
                    )
                };
                let text = format!("Received {} for `{what}` from {who}.", files_word(files));
                if failed == 0 {
                    self.ui.info(text);
                } else {
                    self.ui.warn(format!(
                        "{text} {failed} could not be written; open the log folder in \
                         Settings to see why."
                    ));
                }
            }
            NetEvent::Log(text) => self.ui.info(text),
            NetEvent::ProjectsChanged(text) => {
                self.sync_projects().await;
                self.ui.info(text);
            }
        }
    }
}

pub(super) fn files_word(n: u64) -> String {
    match n {
        1 => "1 file".to_string(),
        n => format!("{n} files"),
    }
}
