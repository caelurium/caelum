use super::{Pending, Workspace};
use crate::application::{Command, Entry, EntryKind, Reply, SessionKey};
use caelum_protocol::{ApprovalDecision, ApprovalPrompt, ApprovalRule};
use std::collections::HashMap;

const MAX_RULES: usize = 32;

enum Response {
    Issued,
    RetryManually,
}

#[derive(Default)]
pub(super) struct State {
    rules: HashMap<SessionKey, Vec<ApprovalRule>>,
    responses: HashMap<String, Response>,
}

#[derive(Clone, Copy)]
enum Decision {
    Grant,
    Automatic,
    Once(ApprovalDecision),
}

pub(super) struct Attempt {
    slot: usize,
    owner: SessionKey,
    token: String,
    rule: ApprovalRule,
    decision: Decision,
}

impl Workspace {
    pub(super) fn forget_approval_checkout(&mut self, project: &str) {
        self.policy
            .rules
            .retain(|key, _| key.scope.project != project);
    }

    pub fn approval_rules(&self, owner: &SessionKey) -> &[ApprovalRule] {
        self.policy.rules.get(owner).map_or(&[], Vec::as_slice)
    }

    pub fn revoke_approval_rules(&mut self, owner: &SessionKey, rule: Option<&ApprovalRule>) {
        if let Some(rules) = self.policy.rules.get_mut(owner) {
            rules.retain(|r| rule.is_some_and(|item| item != r));
            if rules.is_empty() {
                self.policy.rules.remove(owner);
            }
        }
        for pending in self.pending.values_mut() {
            if let Pending::ScopedApproval(a) = pending {
                if &a.owner == owner
                    && matches!(a.decision, Decision::Grant)
                    && rule.is_none_or(|r| r == &a.rule)
                {
                    a.decision = Decision::Once(ApprovalDecision::AllowOnce);
                }
            }
        }
    }

    fn approval_owner(&self, prompt: &ApprovalPrompt) -> Option<(usize, SessionKey)> {
        let id = prompt.session_id.as_ref()?;
        let slot = self.sessions.iter().position(|s| {
            !s.archived()
                && !s.archiving()
                && !s.forgotten()
                && !s.disconnected
                && s.scope.project == prompt.project_path
                && s.scope.provider.as_str() == prompt.provider
                && s.remote_id.as_ref() == Some(id)
        })?;
        Some((slot, self.sessions[slot].key()?))
    }

    pub fn can_allow_similar(&self, prompt: &ApprovalPrompt) -> bool {
        !prompt.attempt.is_empty()
            && prompt.repeatable.is_some()
            && self
                .approval_owner(prompt)
                .is_some_and(|(_, owner)| self.approval_rules(&owner).len() < MAX_RULES)
            && self.approvals.contains(prompt)
            && !self.scoped_approval_pending(prompt)
    }

    pub fn allow_similar(&mut self, prompt: ApprovalPrompt) {
        if self.can_allow_similar(&prompt) {
            self.answer_scoped(&prompt, Decision::Grant);
        }
    }

    fn answer_scoped(&mut self, prompt: &ApprovalPrompt, decision: Decision) -> bool {
        let Some(rule) = prompt.repeatable.as_ref() else {
            return false;
        };
        let Some(owner) = self.approval_owner(prompt) else {
            return false;
        };
        if self.approvals.contains(prompt) && !self.scoped_approval_pending(prompt) {
            self.issue_scoped(prompt.clone(), owner, rule.clone(), decision);
        }
        true
    }

    pub fn approve(&mut self, prompt: ApprovalPrompt, decision: ApprovalDecision) {
        if self.answer_scoped(&prompt, Decision::Once(decision)) {
            return;
        }
        let owner = Pending::Request {
            provider: prompt.provider.clone(),
            project: prompt.project_path.clone(),
            session: prompt.session_id.clone(),
        };
        self.request(Command::Approve(prompt, decision), owner);
    }

    pub fn scoped_approval_pending(&self, prompt: &ApprovalPrompt) -> bool {
        matches!(
            self.policy.responses.get(&prompt.attempt),
            Some(Response::Issued)
        )
    }

    fn issue_scoped(
        &mut self,
        prompt: ApprovalPrompt,
        (slot, owner): (usize, SessionKey),
        rule: ApprovalRule,
        decision: Decision,
    ) {
        let token = prompt.attempt.clone();
        self.policy
            .responses
            .insert(token.clone(), Response::Issued);
        let reply = match decision {
            Decision::Once(d) => d,
            _ => ApprovalDecision::AllowOnce,
        };
        self.request(
            Command::Approve(prompt, reply),
            Pending::ScopedApproval(Attempt {
                slot,
                owner,
                token,
                rule,
                decision,
            }),
        );
    }

    pub(super) fn apply_rules(&mut self, prompts: &[ApprovalPrompt]) {
        self.policy
            .responses
            .retain(|id, _| prompts.iter().any(|p| p.attempt == *id));
        for p in prompts {
            let Some(rule) = &p.repeatable else { continue };
            if p.attempt.is_empty() || self.policy.responses.contains_key(&p.attempt) {
                continue;
            }
            if let Some(owner) = self.approval_owner(p) {
                if self.approval_rules(&owner.1).contains(rule) {
                    self.issue_scoped(p.clone(), owner, rule.clone(), Decision::Automatic);
                }
            }
        }
    }

    pub(super) fn finish_scoped(&mut self, a: Attempt, result: Result<Reply, String>) {
        let Some(s) = self
            .sessions
            .get_mut(a.slot)
            .filter(|s| !s.forgotten() && s.key().as_ref() == Some(&a.owner))
        else {
            return;
        };
        if !matches!(result, Ok(Reply::Done)) {
            if let Some(state) = self.policy.responses.get_mut(&a.token) {
                *state = Response::RetryManually;
            }
            s.error = Some(
                result
                    .err()
                    .unwrap_or_else(|| "Approval was not confirmed.".into()),
            );
            return;
        }
        match a.decision {
            Decision::Grant if !s.archived() => {
                let rules = self.policy.rules.entry(a.owner).or_default();
                if rules.len() < MAX_RULES && !rules.contains(&a.rule) {
                    rules.push(a.rule);
                }
                self.apply_rules(&self.approvals.clone());
            }
            Decision::Automatic => {
                let mut entry = Entry::new(
                    format!("caelum-approval-{}", a.token),
                    EntryKind::Status,
                    a.rule.to_string(),
                    true,
                );
                entry.name = "session-approval".into();
                entry.started_at = Some(crate::application::presentation::now());
                s.conversation.entries.push(entry);
                s.revision += 1;
            }
            _ => {}
        }
    }
}
