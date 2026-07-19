use crate::error::{AllocationConflictKind, AllocationPurpose, TaskCreationError};
use kernel_contracts::{
    validate_json, ChildTaskMaterializationAllocationV1, RootTaskCreateAllocationV2,
};
use std::collections::HashSet;
use uuid::Uuid;

const ROOT_ALLOCATION_SCHEMA: &str =
    "https://schemas.shittim.local/task/root_task_create_allocation/v2";
const CHILD_ALLOCATION_SCHEMA: &str =
    "https://schemas.shittim.local/task/child_task_materialization_allocation/v1";

/// Complete external UUID snapshot required to validate a root allocation.
///
/// Every relationship slot is an explicitly typed UUID. Callers must parse wire
/// text before constructing the snapshot, so accepted spellings cannot vary
/// between relationship validation paths.
///
/// ```compile_fail
/// use kernel_task_creation::RootTaskCreateExternalUuidRefsV1;
///
/// let _missing_relationship_slots = RootTaskCreateExternalUuidRefsV1 {
///     command_request_id: uuid::Uuid::nil(),
/// };
/// ```
///
/// ```compile_fail
/// use kernel_task_creation::RootTaskCreateExternalUuidRefsV1;
///
/// let _free_uuid_bag = RootTaskCreateExternalUuidRefsV1 {
///     command_request_id: uuid::Uuid::nil(),
///     delegation_ref: None,
///     parent_origin_refs: vec![],
///     external_uuid_refs: vec![uuid::Uuid::nil()],
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootTaskCreateExternalUuidRefsV1 {
    /// Parsed command envelope request UUID.
    pub command_request_id: Uuid,
    /// Parsed selected delegation UUID, when present.
    pub delegation_ref: Option<Uuid>,
    /// Complete parsed parent content-origin UUIDs, preserving caller order and duplicates.
    pub parent_origin_refs: Vec<Uuid>,
}

/// Complete external UUID snapshot required to validate a child allocation.
///
/// ```compile_fail
/// use kernel_task_creation::ChildTaskMaterializationExternalUuidRefsV1;
///
/// let _free_uuid_bag = ChildTaskMaterializationExternalUuidRefsV1 {
///     parent_task_id: uuid::Uuid::nil(),
///     action_id: uuid::Uuid::nil(),
///     permission_decision_id: uuid::Uuid::nil(),
///     approval_resolution_ref: None,
///     credential_refs: vec![],
///     challenge_refs: vec![],
///     delegation_ref: None,
///     parent_origin_refs: vec![],
///     external_uuid_refs: vec![uuid::Uuid::nil()],
/// };
/// ```
///
/// ```compile_fail
/// use kernel_task_creation::ChildTaskMaterializationExternalUuidRefsV1;
///
/// let _missing_required_vec = ChildTaskMaterializationExternalUuidRefsV1 {
///     parent_task_id: uuid::Uuid::nil(),
///     action_id: uuid::Uuid::nil(),
///     permission_decision_id: uuid::Uuid::nil(),
///     approval_resolution_ref: None,
///     credential_refs: vec![],
///     delegation_ref: None,
///     parent_origin_refs: vec![],
/// };
/// ```
///
/// All `Option` and `Vec` fields remain required struct members: absence is an
/// explicit business fact, not a missing free-form bag entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildTaskMaterializationExternalUuidRefsV1 {
    /// Parsed authoritative parent Task UUID.
    pub parent_task_id: Uuid,
    /// Parsed child-create Action UUID.
    pub action_id: Uuid,
    /// Parsed current PermissionDecision UUID.
    pub permission_decision_id: Uuid,
    /// Parsed consumed Approval resolution UUID, when required.
    pub approval_resolution_ref: Option<Uuid>,
    /// Complete parsed credential UUIDs used by the decision.
    pub credential_refs: Vec<Uuid>,
    /// Complete parsed challenge UUIDs used by the decision.
    pub challenge_refs: Vec<Uuid>,
    /// Parsed selected delegation UUID, when present.
    pub delegation_ref: Option<Uuid>,
    /// Complete parsed parent content-origin UUIDs, preserving caller order and duplicates.
    pub parent_origin_refs: Vec<Uuid>,
}

/// Validates root allocation Schema, UUID relationships, and opaque independence.
pub fn validate_root_task_create_allocation(
    allocation: &RootTaskCreateAllocationV2,
    external: &RootTaskCreateExternalUuidRefsV1,
) -> Result<(), TaskCreationError> {
    validate_typed_allocation(
        AllocationPurpose::RootTaskCreate,
        ROOT_ALLOCATION_SCHEMA,
        allocation,
    )?;
    let internal = root_internal_uuids(allocation)?;
    let external = root_external_uuids(external);
    validate_relationships(AllocationPurpose::RootTaskCreate, &internal, &external)?;
    validate_opaque(
        AllocationPurpose::RootTaskCreate,
        &[
            &allocation.correlation_id,
            &allocation.task_created_dedup_key,
        ],
    )
}

/// Validates child allocation Schema, UUID relationships, and opaque independence.
pub fn validate_child_task_materialization_allocation(
    allocation: &ChildTaskMaterializationAllocationV1,
    external: &ChildTaskMaterializationExternalUuidRefsV1,
) -> Result<(), TaskCreationError> {
    validate_typed_allocation(
        AllocationPurpose::ChildTaskMaterialization,
        CHILD_ALLOCATION_SCHEMA,
        allocation,
    )?;
    let internal = child_internal_uuids(allocation)?;
    let external = child_external_uuids(external);
    validate_relationships(
        AllocationPurpose::ChildTaskMaterialization,
        &internal,
        &external,
    )?;
    validate_opaque(
        AllocationPurpose::ChildTaskMaterialization,
        &[
            &allocation.correlation_id,
            &allocation.task_created_dedup_key,
            &allocation.action_state_changed_dedup_key,
        ],
    )
}

fn validate_typed_allocation<T: serde::Serialize>(
    purpose: AllocationPurpose,
    schema_id: &str,
    allocation: &T,
) -> Result<(), TaskCreationError> {
    let value = serde_json::to_value(allocation).map_err(TaskCreationError::InternalJson)?;
    validate_json(schema_id, &value)
        .map_err(|source| TaskCreationError::InvalidAllocationContract { purpose, source })
}

fn root_internal_uuids(
    allocation: &RootTaskCreateAllocationV2,
) -> Result<Vec<Uuid>, TaskCreationError> {
    let purpose = AllocationPurpose::RootTaskCreate;
    [
        ("task_id", &allocation.task_id),
        ("task_scope_id", &allocation.task_scope_id),
        ("content_origin_id", &allocation.content_origin_id),
        ("kernel_receipt_id", &allocation.kernel_receipt_id),
        ("creation_provenance_id", &allocation.creation_provenance_id),
        ("audit_record_id", &allocation.audit_record_id),
        ("task_created_event_id", &allocation.task_created_event_id),
    ]
    .into_iter()
    .map(|(field, value)| parse_internal_uuid(purpose, field, value))
    .collect()
}

fn child_internal_uuids(
    allocation: &ChildTaskMaterializationAllocationV1,
) -> Result<Vec<Uuid>, TaskCreationError> {
    let purpose = AllocationPurpose::ChildTaskMaterialization;
    [
        ("child_task_id", &allocation.child_task_id),
        ("task_scope_id", &allocation.task_scope_id),
        ("content_origin_id", &allocation.content_origin_id),
        ("kernel_receipt_id", &allocation.kernel_receipt_id),
        ("creation_provenance_id", &allocation.creation_provenance_id),
        ("verification_result_id", &allocation.verification_result_id),
        ("audit_record_id", &allocation.audit_record_id),
        ("task_created_event_id", &allocation.task_created_event_id),
        (
            "action_state_changed_event_id",
            &allocation.action_state_changed_event_id,
        ),
        ("action_transition_id", &allocation.action_transition_id),
    ]
    .into_iter()
    .map(|(field, value)| parse_internal_uuid(purpose, field, value))
    .collect()
}

fn root_external_uuids(input: &RootTaskCreateExternalUuidRefsV1) -> Vec<Uuid> {
    let mut values = vec![input.command_request_id];
    values.extend(input.delegation_ref);
    values.extend(input.parent_origin_refs.iter().copied());
    values
}

fn child_external_uuids(input: &ChildTaskMaterializationExternalUuidRefsV1) -> Vec<Uuid> {
    let mut values = vec![
        input.parent_task_id,
        input.action_id,
        input.permission_decision_id,
    ];
    values.extend(input.approval_resolution_ref);
    values.extend(input.credential_refs.iter().copied());
    values.extend(input.challenge_refs.iter().copied());
    values.extend(input.delegation_ref);
    values.extend(input.parent_origin_refs.iter().copied());
    values
}

fn parse_internal_uuid(
    purpose: AllocationPurpose,
    field: &'static str,
    value: &str,
) -> Result<Uuid, TaskCreationError> {
    Uuid::parse_str(value).map_err(|_| TaskCreationError::InvalidUuid { purpose, field })
}

fn validate_relationships(
    purpose: AllocationPurpose,
    internal: &[Uuid],
    external: &[Uuid],
) -> Result<(), TaskCreationError> {
    let unique: HashSet<_> = internal.iter().copied().collect();
    if unique.len() != internal.len() {
        return Err(conflict(
            purpose,
            AllocationConflictKind::DuplicateInternalUuid,
        ));
    }
    let external: HashSet<_> = external.iter().copied().collect();
    if internal.iter().any(|value| external.contains(value)) {
        return Err(conflict(
            purpose,
            AllocationConflictKind::ExternalUuidCollision,
        ));
    }
    Ok(())
}

fn validate_opaque(purpose: AllocationPurpose, values: &[&str]) -> Result<(), TaskCreationError> {
    let unique: HashSet<_> = values.iter().copied().collect();
    if unique.len() != values.len() {
        return Err(conflict(purpose, AllocationConflictKind::DuplicateOpaque));
    }
    Ok(())
}

fn conflict(purpose: AllocationPurpose, kind: AllocationConflictKind) -> TaskCreationError {
    TaskCreationError::AllocationConflict { purpose, kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel_contracts::{
        ChildTaskMaterializationAllocationV1SchemaVersion, RootTaskCreateAllocationV2SchemaVersion,
    };

    #[test]
    fn root_rejects_internal_external_and_opaque_conflicts() {
        let allocation = root_allocation();
        let external = root_external();
        validate_root_task_create_allocation(&allocation, &external).unwrap();

        let mut duplicate = allocation.clone();
        duplicate.task_scope_id = duplicate.task_id.clone();
        assert_conflict(
            validate_root_task_create_allocation(&duplicate, &external).unwrap_err(),
            AllocationConflictKind::DuplicateInternalUuid,
        );

        let collision = RootTaskCreateExternalUuidRefsV1 {
            command_request_id: Uuid::parse_str(&allocation.task_id).unwrap(),
            ..external.clone()
        };
        assert_conflict(
            validate_root_task_create_allocation(&allocation, &collision).unwrap_err(),
            AllocationConflictKind::ExternalUuidCollision,
        );

        let mut duplicate_opaque = allocation;
        duplicate_opaque.task_created_dedup_key = duplicate_opaque.correlation_id.clone();
        assert_conflict(
            validate_root_task_create_allocation(&duplicate_opaque, &external).unwrap_err(),
            AllocationConflictKind::DuplicateOpaque,
        );
    }

    #[test]
    fn child_rejects_complete_external_snapshot_collisions() {
        let allocation = child_allocation();
        let mut external = child_external();
        validate_child_task_materialization_allocation(&allocation, &external).unwrap();
        external
            .challenge_refs
            .push(Uuid::parse_str(&allocation.action_transition_id).unwrap());
        assert_conflict(
            validate_child_task_materialization_allocation(&allocation, &external).unwrap_err(),
            AllocationConflictKind::ExternalUuidCollision,
        );
    }

    #[test]
    fn external_uuid_canonicalization_happens_at_caller_parse_boundary() {
        let uppercase = Uuid::parse_str("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAA1").unwrap();
        let lowercase = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1").unwrap();
        assert_eq!(uppercase, lowercase);
        let external = RootTaskCreateExternalUuidRefsV1 {
            command_request_id: uppercase,
            delegation_ref: None,
            parent_origin_refs: vec![],
        };
        assert_eq!(
            external.command_request_id.to_string(),
            lowercase.to_string()
        );
    }

    #[test]
    fn allocation_schema_is_always_checked_first() {
        let mut allocation = root_allocation();
        allocation.correlation_id.clear();
        assert!(matches!(
            validate_root_task_create_allocation(&allocation, &root_external()).unwrap_err(),
            TaskCreationError::InvalidAllocationContract {
                purpose: AllocationPurpose::RootTaskCreate,
                ..
            }
        ));
    }

    fn assert_conflict(error: TaskCreationError, expected: AllocationConflictKind) {
        assert!(matches!(
            error,
            TaskCreationError::AllocationConflict { kind, .. } if kind == expected
        ));
    }

    fn root_allocation() -> RootTaskCreateAllocationV2 {
        RootTaskCreateAllocationV2 {
            audit_record_id: id(6),
            content_origin_id: id(3),
            correlation_id: "root-correlation".to_owned(),
            creation_provenance_id: id(5),
            kernel_receipt_id: id(4),
            schema_version: RootTaskCreateAllocationV2SchemaVersion,
            task_created_dedup_key: "root-dedup".to_owned(),
            task_created_event_id: id(7),
            task_id: id(1),
            task_scope_id: id(2),
        }
    }

    fn child_allocation() -> ChildTaskMaterializationAllocationV1 {
        ChildTaskMaterializationAllocationV1 {
            action_state_changed_dedup_key: "action-dedup".to_owned(),
            action_state_changed_event_id: id(9),
            action_transition_id: id(10),
            audit_record_id: id(7),
            child_task_id: id(1),
            content_origin_id: id(3),
            correlation_id: "child-correlation".to_owned(),
            creation_provenance_id: id(5),
            kernel_receipt_id: id(4),
            schema_version: ChildTaskMaterializationAllocationV1SchemaVersion,
            task_created_dedup_key: "task-dedup".to_owned(),
            task_created_event_id: id(8),
            task_scope_id: id(2),
            verification_result_id: id(6),
        }
    }

    fn root_external() -> RootTaskCreateExternalUuidRefsV1 {
        RootTaskCreateExternalUuidRefsV1 {
            command_request_id: external_id(1),
            delegation_ref: Some(external_id(2)),
            parent_origin_refs: vec![external_id(3), external_id(3)],
        }
    }

    fn child_external() -> ChildTaskMaterializationExternalUuidRefsV1 {
        ChildTaskMaterializationExternalUuidRefsV1 {
            parent_task_id: external_id(1),
            action_id: external_id(2),
            permission_decision_id: external_id(3),
            approval_resolution_ref: Some(external_id(4)),
            credential_refs: vec![external_id(5), external_id(6)],
            challenge_refs: vec![external_id(7)],
            delegation_ref: Some(external_id(8)),
            parent_origin_refs: vec![external_id(9), external_id(9)],
        }
    }

    fn id(index: u128) -> String {
        Uuid::from_u128(0x11111111_1111_4111_8111_000000000000 + index).to_string()
    }

    fn external_id(index: u128) -> Uuid {
        Uuid::from_u128(0xaaaaaaaa_aaaa_4aaa_8aaa_000000000000 + index)
    }
}
