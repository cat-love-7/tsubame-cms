variable "region" {
  description = "Where the state bucket lives. Keep it the same as `var.region` in `infra/`."
  type        = string
  default     = "eu-west-1"
}

variable "project" {
  description = "Name prefix, as in `infra/`."
  type        = string
  default     = "sl-cms"
}

variable "environment" {
  description = "Tagged on the bucket, as in `infra/`; deployments are told apart by the state key."
  type        = string
  default     = "staging"
}

variable "state_bucket" {
  description = <<-EOT
    Name of the bucket that holds the remote state. S3 names are global, so this has to be
    something nobody else has taken - an account id or an organisation name is the usual way.
    Terraform cannot invent it: `infra/` has to be able to name it before it exists.
  EOT
  type        = string
}
