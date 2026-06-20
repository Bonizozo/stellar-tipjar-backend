DROP INDEX IF EXISTS idx_saga_step_executions_executed_at;
DROP INDEX IF EXISTS idx_saga_step_executions_workflow_id;
DROP INDEX IF EXISTS idx_saga_workflows_created_at;
DROP INDEX IF EXISTS idx_saga_workflows_status;
DROP TABLE IF EXISTS saga_step_executions;
DROP TABLE IF EXISTS saga_workflows;
