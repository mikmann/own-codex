ALTER TABLE extended_workflow_validations ADD COLUMN reviewer_verdict TEXT;
ALTER TABLE extended_workflow_validations ADD COLUMN gate_status TEXT NOT NULL DEFAULT 'excluded';
ALTER TABLE extended_workflow_validations ADD COLUMN validation_error_code TEXT;
ALTER TABLE extended_workflow_validations ADD COLUMN validation_message TEXT;
ALTER TABLE extended_workflow_validations ADD COLUMN finding_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE extended_workflow_validations ADD COLUMN max_severity TEXT;
ALTER TABLE extended_workflow_validations ADD COLUMN change_digest_sha256 TEXT;
