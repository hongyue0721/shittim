use crate::canonical::{finalize_projection, CanonicalProjection};
use crate::child_delta::{require_hash, require_nonempty, require_positive, to_i64, uuid_text};
use crate::AuthorizationProjectionError;
use kernel_contracts::{
    SideEffectClass, SubjectProjectionV1, SubjectProjectionV1OperationSchemaVersion,
    SubjectProjectionV1PlanRevisionSchemaVersion, SubjectProjectionV1TaskProposalSchemaVersion,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const SCHEMA_ID: &str = "https://schemas.shittim.local/policy/subject_projection/v1";

/// Caller-injected authoritative Approval subject facts.
///
/// The enum mirrors the three `SubjectV2` branches exactly. JSON serialization exists for
/// official fixtures only; production callers use the typed variants directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "subject_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectProjectionFactsV1 {
    /// Executable operation bound to current Task, Action, and PermissionDecision revisions.
    Operation {
        /// Current Task UUID.
        task_id: Uuid,
        /// Current positive Task revision.
        task_revision: u64,
        /// Current Task plan version.
        task_plan_version: u64,
        /// Current Action UUID.
        action_id: Uuid,
        /// Current positive Action revision.
        action_revision: u64,
        /// Current PermissionDecision UUID.
        permission_decision_ref: Uuid,
        /// Current positive PermissionDecision revision.
        permission_decision_revision: u64,
        /// Current Policy set revision.
        policy_set_revision: u64,
        /// Lowercase material authorization fingerprint.
        material_authorization_fingerprint: String,
        /// Capability ID.
        capability_id: String,
        /// Operation name.
        operation: String,
        /// Side-effect class.
        side_effect_class: SideEffectClass,
        /// Lowercase normalized resource set hash.
        resource_refs_hash: String,
        /// Lowercase normalized key parameters hash.
        key_params_hash: String,
    },
    /// Candidate Task proposal not yet materialized as an executable Action.
    TaskProposal {
        /// Candidate Task UUID.
        candidate_task_id: Uuid,
        /// Current positive candidate revision.
        candidate_revision: u64,
        /// Lowercase proposal hash.
        proposal_hash: String,
        /// Stable proposer Actor reference.
        proposer_actor_ref: String,
        /// Lowercase candidate TaskScope hash.
        task_scope_hash: String,
        /// Delegation UUID, if applicable.
        delegation_ref: Option<Uuid>,
        /// Current Policy set revision.
        policy_set_revision: u64,
    },
    /// Proposed Task plan revision.
    PlanRevision {
        /// Current Task UUID.
        task_id: Uuid,
        /// Current positive Task revision.
        task_revision: u64,
        /// Base plan version.
        base_plan_version: u64,
        /// Proposed plan version.
        proposed_plan_version: u64,
        /// Lowercase proposed plan hash.
        proposed_plan_hash: String,
        /// Current Policy set revision.
        policy_set_revision: u64,
    },
}

/// Constructs, Schema-validates, canonicalizes, and hashes `SubjectProjectionV1`.
pub fn project_subject_projection(
    facts: SubjectProjectionFactsV1,
) -> Result<CanonicalProjection<SubjectProjectionV1>, AuthorizationProjectionError> {
    let value = match facts {
        SubjectProjectionFactsV1::Operation {
            task_id,
            task_revision,
            task_plan_version,
            action_id,
            action_revision,
            permission_decision_ref,
            permission_decision_revision,
            policy_set_revision,
            material_authorization_fingerprint,
            capability_id,
            operation,
            side_effect_class,
            resource_refs_hash,
            key_params_hash,
        } => {
            require_positive(task_revision, "task_revision")?;
            require_positive(action_revision, "action_revision")?;
            require_positive(permission_decision_revision, "permission_decision_revision")?;
            require_nonempty(&capability_id, "capability_id")?;
            require_nonempty(&operation, "operation")?;
            require_hash(
                &material_authorization_fingerprint,
                "material_authorization_fingerprint",
            )?;
            require_hash(&resource_refs_hash, "resource_refs_hash")?;
            require_hash(&key_params_hash, "key_params_hash")?;
            SubjectProjectionV1::Operation {
                action_id: uuid_text(action_id),
                action_revision: to_i64(action_revision, "action_revision")?,
                capability_id,
                key_params_hash,
                material_authorization_fingerprint,
                operation,
                permission_decision_ref: uuid_text(permission_decision_ref),
                permission_decision_revision: to_i64(
                    permission_decision_revision,
                    "permission_decision_revision",
                )?,
                policy_set_revision: to_i64(policy_set_revision, "policy_set_revision")?,
                resource_refs_hash,
                schema_version: SubjectProjectionV1OperationSchemaVersion,
                side_effect_class,
                task_id: uuid_text(task_id),
                task_plan_version: to_i64(task_plan_version, "task_plan_version")?,
                task_revision: to_i64(task_revision, "task_revision")?,
            }
        }
        SubjectProjectionFactsV1::TaskProposal {
            candidate_task_id,
            candidate_revision,
            proposal_hash,
            proposer_actor_ref,
            task_scope_hash,
            delegation_ref,
            policy_set_revision,
        } => {
            require_positive(candidate_revision, "candidate_revision")?;
            require_hash(&proposal_hash, "proposal_hash")?;
            require_nonempty(&proposer_actor_ref, "proposer_actor_ref")?;
            require_hash(&task_scope_hash, "task_scope_hash")?;
            SubjectProjectionV1::TaskProposal {
                candidate_revision: to_i64(candidate_revision, "candidate_revision")?,
                candidate_task_id: uuid_text(candidate_task_id),
                delegation_ref: delegation_ref.map(uuid_text),
                policy_set_revision: to_i64(policy_set_revision, "policy_set_revision")?,
                proposal_hash,
                proposer_actor_ref,
                schema_version: SubjectProjectionV1TaskProposalSchemaVersion,
                task_scope_hash,
            }
        }
        SubjectProjectionFactsV1::PlanRevision {
            task_id,
            task_revision,
            base_plan_version,
            proposed_plan_version,
            proposed_plan_hash,
            policy_set_revision,
        } => {
            require_positive(task_revision, "task_revision")?;
            require_hash(&proposed_plan_hash, "proposed_plan_hash")?;
            SubjectProjectionV1::PlanRevision {
                base_plan_version: to_i64(base_plan_version, "base_plan_version")?,
                policy_set_revision: to_i64(policy_set_revision, "policy_set_revision")?,
                proposed_plan_hash,
                proposed_plan_version: to_i64(proposed_plan_version, "proposed_plan_version")?,
                schema_version: SubjectProjectionV1PlanRevisionSchemaVersion,
                task_id: uuid_text(task_id),
                task_revision: to_i64(task_revision, "task_revision")?,
            }
        }
    };
    finalize_projection(SCHEMA_ID, value)
}
