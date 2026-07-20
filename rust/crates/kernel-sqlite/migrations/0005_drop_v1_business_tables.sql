-- kernel-sqlite migration phase: schema
-- Fresh-baseline cleanup (ADR-0009 / V2InitialBuildActive slice 3c).
-- 0001-0004 asset bytes stay stable. This migration only runs after open-time and
-- transform-time checks prove there is no v1 business data to preserve.
-- Non-empty legacy tables or outbox schema_version=1 rows are rejected with
-- reinitialize-required; Kernel never auto-wipes or migrates those rows.
--
-- Drops dead v1 write-path tables so a fresh baseline has no unused legacy stores:
-- content_origins(+parent_refs), audit_records, task_create_idempotency.

DROP TRIGGER IF EXISTS content_origins_immutable_update;
DROP TRIGGER IF EXISTS content_origins_immutable_delete;
DROP TRIGGER IF EXISTS content_origin_parent_refs_no_late_insert;
DROP TRIGGER IF EXISTS content_origin_parent_refs_immutable_update;
DROP TRIGGER IF EXISTS content_origin_parent_refs_immutable_delete;
DROP TRIGGER IF EXISTS audit_records_immutable_update;
DROP TRIGGER IF EXISTS audit_records_immutable_delete;

DROP INDEX IF EXISTS content_origins_kind_idx;
DROP INDEX IF EXISTS content_origins_received_at_idx;
DROP INDEX IF EXISTS content_origin_parent_refs_parent_idx;
DROP INDEX IF EXISTS audit_records_id_idx;
DROP INDEX IF EXISTS audit_records_type_idx;
DROP INDEX IF EXISTS audit_records_time_idx;
DROP INDEX IF EXISTS audit_records_task_idx;
DROP INDEX IF EXISTS audit_records_action_idx;

DROP TABLE IF EXISTS content_origin_parent_refs;
DROP TABLE IF EXISTS content_origins;
DROP TABLE IF EXISTS task_create_idempotency;
DROP TABLE IF EXISTS audit_records;
