output "state_bucket" {
  description = "The bucket that `infra/backend.hcl` names."
  value       = aws_s3_bucket.state.id
}
