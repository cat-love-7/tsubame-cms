variable "region" {
  description = "Where everything lives. Cognito, DynamoDB, S3 and the function are all regional."
  type        = string
  default     = "eu-west-1"
}

variable "project" {
  description = "Name prefix for every resource, so two deployments in one account can differ."
  type        = string
  default     = "sl-cms"
}

variable "environment" {
  description = "Which deployment this is; part of the resource names."
  type        = string
  default     = "staging"
}

variable "jwt_secret" {
  description = <<-EOT
    Signs preview links and password-reset links — the same secret the on-premises deployment
    uses. Cognito issues the sign-in tokens, so this one no longer signs those.
  EOT
  type        = string
  sensitive   = true
}

variable "bootstrap_admin_usernames" {
  description = <<-EOT
    Who may become the first administrators by signing in (see doc/aws-plan.md). Comma-joined
    names or addresses, normalized the way accounts are stored.
  EOT
  type        = list(string)
  default     = []
}

variable "cors_allowed_origins" {
  description = "Origins the browser may call the API from."
  type        = list(string)
  default     = ["http://localhost:4200"]
}

variable "cognito_domain_prefix" {
  description = <<-EOT
    The prefix of the hosted sign-in page's address. Cognito requires it to be unique across
    *all* accounts in the region, so a second deployment of this stack usually has to change it.
  EOT
  type        = string
  default     = ""
}

variable "function_zip" {
  description = <<-EOT
    The built Lambda artifact. `scripts/build-lambda.sh` writes it; the file only has to exist
    when planning or applying, not for `validate`.
  EOT
  type        = string
  default     = "build/sl-cms-aws.zip"
}

variable "function_memory_mb" {
  description = "How much memory the function gets; CPU scales with it."
  type        = number
  default     = 512
}

variable "function_timeout_seconds" {
  description = "Long enough for a publish, short enough that a stuck request gives up."
  type        = number
  default     = 30
}

variable "log_retention_days" {
  description = "How long the function's logs are kept."
  type        = number
  default     = 30
}

variable "point_in_time_recovery" {
  description = "DynamoDB continuous backups. Worth the cost for content, off in a scratch deployment."
  type        = bool
  default     = true
}

variable "deletion_protection" {
  description = "Refuse to destroy the table. Off for a deployment that is still being built."
  type        = bool
  default     = false
}

variable "waf_rate_limit" {
  description = <<-EOT
    Requests per five minutes from one address to the sign-in endpoint before it is blocked.
    A per-account lockout is Cognito's; this is the volume case, which Cognito's documentation
    points at WAF for.
  EOT
  type        = number
  default     = 2000
}

variable "webhook_urls" {
  description = "Where content events are delivered. Empty means webhooks are off."
  type        = list(string)
  default     = []
}

variable "webhook_secret" {
  description = "Signs webhook deliveries (HMAC-SHA256). Empty delivers unsigned."
  type        = string
  default     = ""
  sensitive   = true
}
