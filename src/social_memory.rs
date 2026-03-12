use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::AgentId;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataSensitivity {
    Public,
    Internal,
    Confidential,
    Secret,
}

impl Default for DataSensitivity {
    fn default() -> Self {
        Self::Internal
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromiseStatus {
    Pending,
    Kept,
    Broken,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromiseRecord {
    pub id: String,
    pub promiser: AgentId,
    pub beneficiary: Option<AgentId>,
    pub task_key: String,
    pub summary: String,
    pub sensitivity: DataSensitivity,
    pub promised_at: u64,
    pub due_by: Option<u64>,
    pub resolved_at: Option<u64>,
    pub status: PromiseStatus,
    pub delivered_by: Option<AgentId>,
    pub quality_score: Option<f64>,
    pub met_deadline: Option<bool>,
    pub safe_for_sensitive_data: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CapabilityEvidence {
    pub attempts: u64,
    pub successes: u64,
    pub avg_quality: f64,
    pub last_updated: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentReputation {
    pub agent_id: AgentId,
    pub successful_deliveries: u64,
    pub failed_deliveries: u64,
    pub on_time_deliveries: u64,
    pub missed_deadlines: u64,
    pub promises_kept: u64,
    pub promises_broken: u64,
    pub safe_shares: u64,
    pub unsafe_shares: u64,
    pub total_quality: f64,
    pub total_observations: u64,
    pub capability_profiles: HashMap<String, CapabilityEvidence>,
    pub last_updated: u64,
}

impl AgentReputation {
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            successful_deliveries: 0,
            failed_deliveries: 0,
            on_time_deliveries: 0,
            missed_deadlines: 0,
            promises_kept: 0,
            promises_broken: 0,
            safe_shares: 0,
            unsafe_shares: 0,
            total_quality: 0.0,
            total_observations: 0,
            capability_profiles: HashMap::new(),
            last_updated: 0,
        }
    }

    pub fn avg_quality(&self) -> f64 {
        if self.total_observations == 0 {
            0.5
        } else {
            (self.total_quality / self.total_observations as f64).clamp(0.0, 1.0)
        }
    }

    pub fn reliability_score(&self) -> f64 {
        let kept = self.successful_deliveries + self.promises_kept;
        let broken = self.failed_deliveries + self.promises_broken;
        if kept + broken == 0 {
            0.5
        } else {
            kept as f64 / (kept + broken) as f64
        }
    }

    pub fn timeliness_score(&self) -> f64 {
        let total = self.on_time_deliveries + self.missed_deadlines;
        if total == 0 {
            0.5
        } else {
            self.on_time_deliveries as f64 / total as f64
        }
    }

    pub fn security_score(&self) -> f64 {
        let total = self.safe_shares + self.unsafe_shares;
        if total == 0 {
            0.5
        } else {
            self.safe_shares as f64 / total as f64
        }
    }

    pub fn capability_score(&self, task_key: &str) -> Option<f64> {
        self.capability_profiles.get(task_key).map(|e| {
            if e.attempts == 0 {
                0.5
            } else {
                let success_rate = e.successes as f64 / e.attempts as f64;
                (0.7 * success_rate + 0.3 * e.avg_quality).clamp(0.0, 1.0)
            }
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CollaborationStats {
    pub successful_projects: u64,
    pub failed_projects: u64,
    pub avg_quality: f64,
    pub last_updated: u64,
}

impl CollaborationStats {
    pub fn score(&self) -> f64 {
        let total = self.successful_projects + self.failed_projects;
        if total == 0 {
            0.5
        } else {
            let success = self.successful_projects as f64 / total as f64;
            (0.7 * success + 0.3 * self.avg_quality).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharePolicy {
    pub owner: AgentId,
    pub target: AgentId,
    pub max_sensitivity: DataSensitivity,
    pub min_security_score: f64,
    pub notes: Option<String>,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationCandidate {
    pub agent_id: AgentId,
    pub score: f64,
    pub components: DelegationScoreComponents,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationScoreComponents {
    pub observed_score: f64,
    pub capability_score: f64,
    pub collaboration_score: f64,
    pub jw_prior: f64,
    pub evidence_weight: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocialMemory {
    pub reputations: HashMap<AgentId, AgentReputation>,
    pub promises: HashMap<String, PromiseRecord>,
    pub collaborations: HashMap<(AgentId, AgentId), CollaborationStats>,
    pub share_policies: HashMap<(AgentId, AgentId), SharePolicy>,
    pub next_promise_id: u64,
}

impl SocialMemory {
    pub fn ensure_agent(&mut self, agent_id: AgentId) -> &mut AgentReputation {
        self.reputations
            .entry(agent_id)
            .or_insert_with(|| AgentReputation::new(agent_id))
    }

    pub fn record_promise(
        &mut self,
        promiser: AgentId,
        beneficiary: Option<AgentId>,
        task_key: impl Into<String>,
        summary: impl Into<String>,
        sensitivity: DataSensitivity,
        promised_at: u64,
        due_by: Option<u64>,
    ) -> String {
        let id = format!("promise-{}", self.next_promise_id);
        self.next_promise_id += 1;
        self.ensure_agent(promiser).last_updated = promised_at;
        let record = PromiseRecord {
            id: id.clone(),
            promiser,
            beneficiary,
            task_key: task_key.into(),
            summary: summary.into(),
            sensitivity,
            promised_at,
            due_by,
            resolved_at: None,
            status: PromiseStatus::Pending,
            delivered_by: None,
            quality_score: None,
            met_deadline: None,
            safe_for_sensitive_data: None,
        };
        self.promises.insert(id.clone(), record);
        id
    }

    pub fn resolve_promise(
        &mut self,
        promise_id: &str,
        status: PromiseStatus,
        delivered_by: Option<AgentId>,
        resolved_at: u64,
        quality_score: Option<f64>,
        met_deadline: Option<bool>,
        safe_for_sensitive_data: Option<bool>,
        collaborators: &[AgentId],
    ) -> Option<()> {
        let (promiser, task_key) = {
            let promise = self.promises.get_mut(promise_id)?;
            promise.status = status.clone();
            promise.delivered_by = delivered_by;
            promise.resolved_at = Some(resolved_at);
            promise.quality_score = quality_score;
            promise.met_deadline = met_deadline;
            promise.safe_for_sensitive_data = safe_for_sensitive_data;
            (promise.promiser, promise.task_key.clone())
        };

        let primary = delivered_by.unwrap_or(promiser);
        self.record_delivery(
            primary,
            &task_key,
            matches!(status, PromiseStatus::Kept),
            quality_score.unwrap_or(0.5),
            met_deadline.unwrap_or(false),
            safe_for_sensitive_data.unwrap_or(true),
            resolved_at,
            collaborators,
        );

        let rep = self.ensure_agent(promiser);
        match status {
            PromiseStatus::Kept => rep.promises_kept += 1,
            PromiseStatus::Broken => rep.promises_broken += 1,
            PromiseStatus::Pending | PromiseStatus::Cancelled => {}
        }
        rep.last_updated = resolved_at;
        Some(())
    }

    pub fn record_delivery(
        &mut self,
        agent_id: AgentId,
        task_key: &str,
        success: bool,
        quality_score: f64,
        on_time: bool,
        safe_for_sensitive_data: bool,
        observed_at: u64,
        collaborators: &[AgentId],
    ) {
        let rep = self.ensure_agent(agent_id);
        rep.total_observations += 1;
        rep.total_quality += quality_score.clamp(0.0, 1.0);
        rep.last_updated = observed_at;

        if success {
            rep.successful_deliveries += 1;
        } else {
            rep.failed_deliveries += 1;
        }
        if on_time {
            rep.on_time_deliveries += 1;
        } else {
            rep.missed_deadlines += 1;
        }
        if safe_for_sensitive_data {
            rep.safe_shares += 1;
        } else {
            rep.unsafe_shares += 1;
        }

        let capability = rep
            .capability_profiles
            .entry(task_key.to_string())
            .or_default();
        capability.attempts += 1;
        if success {
            capability.successes += 1;
        }
        let attempts = capability.attempts as f64;
        capability.avg_quality = ((capability.avg_quality * (attempts - 1.0))
            + quality_score.clamp(0.0, 1.0))
            / attempts;
        capability.last_updated = observed_at;

        for &other in collaborators {
            if other == agent_id {
                continue;
            }
            self.update_collaboration(agent_id, other, success, quality_score, observed_at);
        }
    }

    pub fn set_share_policy(
        &mut self,
        owner: AgentId,
        target: AgentId,
        max_sensitivity: DataSensitivity,
        min_security_score: f64,
        notes: Option<String>,
        updated_at: u64,
    ) {
        self.share_policies.insert(
            (owner, target),
            SharePolicy {
                owner,
                target,
                max_sensitivity,
                min_security_score: min_security_score.clamp(0.0, 1.0),
                notes,
                updated_at,
            },
        );
    }

    pub fn can_share(&self, owner: AgentId, target: AgentId, sensitivity: DataSensitivity) -> bool {
        let Some(policy) = self.share_policies.get(&(owner, target)) else {
            return sensitivity <= DataSensitivity::Internal;
        };
        if sensitivity > policy.max_sensitivity {
            return false;
        }
        let security = self
            .reputations
            .get(&target)
            .map(|r| r.security_score())
            .unwrap_or(0.5);
        security >= policy.min_security_score
    }

    pub fn reputation_score(&self, agent_id: AgentId, jw: f64) -> f64 {
        self.delegation_score(agent_id, None, None, jw).score
    }

    pub fn delegation_score(
        &self,
        candidate: AgentId,
        task_key: Option<&str>,
        requester: Option<AgentId>,
        jw: f64,
    ) -> DelegationCandidate {
        let rep = self.reputations.get(&candidate);
        let observed_score = rep
            .map(|r| {
                let capability = task_key.and_then(|t| r.capability_score(t)).unwrap_or(0.5);
                let capability_weight = if task_key.is_some() { 0.15 } else { 0.0 };
                let base_weight = 1.0 - capability_weight;
                (base_weight
                    * (0.45 * r.reliability_score()
                        + 0.20 * r.timeliness_score()
                        + 0.20 * r.security_score()
                        + 0.15 * r.avg_quality())
                    + capability_weight * capability)
                    .clamp(0.0, 1.0)
            })
            .unwrap_or(0.5);

        let capability_score = rep
            .and_then(|r| task_key.and_then(|t| r.capability_score(t)))
            .unwrap_or(0.5);

        let collaboration_score = requester
            .and_then(|req| self.collaboration_score(req, candidate))
            .unwrap_or(0.5);

        let evidence = rep.map(|r| r.total_observations).unwrap_or(0) as f64;
        let evidence_weight = (evidence / 5.0).min(1.0);
        // Cold starts lean on JW, but once we have delivery evidence the observed record
        // should dominate delegate selection.
        let prior_score =
            (0.65 * jw.clamp(0.0, 1.0) + 0.25 * observed_score + 0.10 * collaboration_score)
                .clamp(0.0, 1.0);
        let evidence_score = (0.90 * observed_score + 0.10 * collaboration_score).clamp(0.0, 1.0);
        let final_score = (evidence_weight * evidence_score
            + (1.0 - evidence_weight) * prior_score)
            .clamp(0.0, 1.0);

        DelegationCandidate {
            agent_id: candidate,
            score: final_score,
            components: DelegationScoreComponents {
                observed_score,
                capability_score,
                collaboration_score,
                jw_prior: jw.clamp(0.0, 1.0),
                evidence_weight,
            },
        }
    }

    pub fn recommend_delegate(
        &self,
        candidates: &[(AgentId, f64)],
        task_key: Option<&str>,
        requester: Option<AgentId>,
        sensitivity: Option<DataSensitivity>,
    ) -> Option<DelegationCandidate> {
        candidates
            .iter()
            .filter(|(agent_id, _)| {
                if let (Some(req), Some(level)) = (requester, sensitivity.clone()) {
                    self.can_share(req, *agent_id, level)
                } else {
                    true
                }
            })
            .map(|(agent_id, jw)| self.delegation_score(*agent_id, task_key, requester, *jw))
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
    }

    pub fn collaboration_score(&self, a: AgentId, b: AgentId) -> Option<f64> {
        let key = ordered_pair(a, b);
        self.collaborations.get(&key).map(|s| s.score())
    }

    fn update_collaboration(
        &mut self,
        a: AgentId,
        b: AgentId,
        success: bool,
        quality_score: f64,
        observed_at: u64,
    ) {
        let stats = self.collaborations.entry(ordered_pair(a, b)).or_default();
        if success {
            stats.successful_projects += 1;
        } else {
            stats.failed_projects += 1;
        }
        let total = (stats.successful_projects + stats.failed_projects) as f64;
        stats.avg_quality =
            ((stats.avg_quality * (total - 1.0)) + quality_score.clamp(0.0, 1.0)) / total;
        stats.last_updated = observed_at;
    }
}

fn ordered_pair(a: AgentId, b: AgentId) -> (AgentId, AgentId) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jw_is_used_as_cold_start_prior() {
        let memory = SocialMemory::default();
        let high = memory.reputation_score(1, 0.9);
        let low = memory.reputation_score(2, 0.2);
        assert!(high > low);
    }

    #[test]
    fn observed_history_overrides_jw_when_evidence_accumulates() {
        let mut memory = SocialMemory::default();
        for t in 0..6 {
            memory.record_delivery(7, "rust", true, 0.95, true, true, t, &[]);
        }
        let proven = memory.reputation_score(7, 0.1);
        let cold = memory.reputation_score(8, 0.9);
        assert!(proven > cold);
    }

    #[test]
    fn resolving_promise_updates_reputation() {
        let mut memory = SocialMemory::default();
        let id = memory.record_promise(
            1,
            Some(2),
            "audit",
            "deliver audit report",
            DataSensitivity::Confidential,
            10,
            Some(20),
        );
        memory.resolve_promise(
            &id,
            PromiseStatus::Kept,
            Some(1),
            18,
            Some(0.9),
            Some(true),
            Some(true),
            &[2],
        );

        let rep = memory.reputations.get(&1).unwrap();
        assert_eq!(rep.promises_kept, 1);
        assert_eq!(rep.successful_deliveries, 1);
    }

    #[test]
    fn share_policy_enforces_sensitivity_boundary() {
        let mut memory = SocialMemory::default();
        memory.record_delivery(2, "ops", true, 0.9, true, true, 1, &[]);
        memory.set_share_policy(1, 2, DataSensitivity::Confidential, 0.7, None, 1);
        assert!(memory.can_share(1, 2, DataSensitivity::Confidential));
        assert!(!memory.can_share(1, 2, DataSensitivity::Secret));
    }

    #[test]
    fn collaboration_history_improves_delegate_selection() {
        let mut memory = SocialMemory::default();
        for t in 0..5 {
            memory.record_delivery(2, "research", true, 0.9, true, true, t, &[1]);
            memory.record_delivery(3, "research", true, 0.7, true, true, t, &[]);
        }
        memory.set_share_policy(1, 2, DataSensitivity::Confidential, 0.5, None, 1);
        memory.set_share_policy(1, 3, DataSensitivity::Confidential, 0.5, None, 1);

        let chosen = memory
            .recommend_delegate(
                &[(2, 0.4), (3, 0.8)],
                Some("research"),
                Some(1),
                Some(DataSensitivity::Confidential),
            )
            .unwrap();
        assert_eq!(chosen.agent_id, 2);
    }

    #[test]
    fn unsafe_candidate_is_filtered_for_confidential_work_even_with_higher_jw() {
        let mut memory = SocialMemory::default();
        for t in 0..4 {
            memory.record_delivery(2, "security-review", true, 0.85, true, true, t, &[1]);
            memory.record_delivery(3, "security-review", true, 0.85, true, false, t, &[]);
        }
        memory.set_share_policy(1, 2, DataSensitivity::Confidential, 0.7, None, 10);
        memory.set_share_policy(1, 3, DataSensitivity::Confidential, 0.7, None, 10);

        let chosen = memory
            .recommend_delegate(
                &[(2, 0.4), (3, 0.95)],
                Some("security-review"),
                Some(1),
                Some(DataSensitivity::Confidential),
            )
            .unwrap();

        assert_eq!(chosen.agent_id, 2);
    }
}
