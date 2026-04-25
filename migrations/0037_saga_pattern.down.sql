-- Rollback saga pattern
DROP INDEX IF EXISTS idx_saga_steps_status;
DROP INDEX IF EXISTS idx_saga_steps_saga_id;
DROP INDEX IF EXISTS idx_saga_executions_type;
DROP INDEX IF EXISTS idx_saga_executions_status;

DROP TABLE IF EXISTS saga_steps;
DROP TABLE IF EXISTS saga_executions;
