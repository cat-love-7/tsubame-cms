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

variable "app_url" {
  description = <<-EOT
    Where the CMS screen is served from, as a browser sees it.

    The identity provider sends the browser back here after a sign-in, and it has to be an address
    it was told about in advance, so this is not something that can be derived from the API's own
    URL. It is also the origin the CMS API is called from, so it normally appears in
    `cors_allowed_origins` as well.
  EOT
  type        = string
  default     = "http://localhost:4200"

  validation {
    condition     = can(regex("^https?://[^/]+$", var.app_url))
    error_message = "app_url is an origin without a trailing slash, for example https://cms.example.com."
  }
}

variable "image_delivery" {
  description = <<-EOT
    How the CMS hands out image URLs.

    `public` (the default) writes the object's own address into content and makes the bucket
    readable: a URL that keeps working, for a site that puts CMS URLs on its pages.

    `presigned` signs every URL instead, and the bucket is left private. Choose it when the site
    **fetches images during a build and serves its own copies** (an SSG that transforms them): the
    signature only has to survive the build, and nobody else can read the bucket. A page that
    cached a CMS URL would carry a dead link once the signature expires, which is why this is not
    the default.
  EOT
  type        = string
  default     = "public"

  validation {
    condition     = contains(["public", "presigned"], var.image_delivery)
    error_message = "image_delivery is 'public' or 'presigned'."
  }
}

variable "image_url_ttl_seconds" {
  description = "How long a signed image URL lasts. Only used when image_delivery is presigned."
  type        = number
  default     = 3600

  validation {
    condition     = var.image_url_ttl_seconds >= 60 && var.image_url_ttl_seconds <= 604800
    error_message = "A signature lasts between a minute and seven days (604800 seconds), as SigV4 allows."
  }
}

variable "cognito_domain_prefix" {
  description = <<-EOT
    The prefix of the hosted sign-in page's address. Cognito requires it to be unique across
    *all* accounts in the region, so a second deployment of this stack usually has to change it.
  EOT
  type        = string
  default     = ""
}

variable "function_architecture" {
  description = <<-EOT
    What the function runs on: `arm64` (Graviton, cheaper per GB-second) or `x86_64`.

    The artifact has to be built for the same architecture -
    `scripts/build-lambda.sh --arch <architecture>` - and the two names are kept in step by
    `local.function_zip` below. A mismatch is only reported when the function is invoked.
  EOT
  type        = string
  default     = "arm64"

  validation {
    condition     = contains(["arm64", "x86_64"], var.function_architecture)
    error_message = "The architecture must be arm64 or x86_64."
  }
}

variable "function_zip" {
  description = <<-EOT
    The built Lambda artifact. Leave empty to use the one `scripts/build-lambda.sh` writes for
    `function_architecture`; the file only has to exist when planning or applying, not for
    `validate`.
  EOT
  type        = string
  default     = ""
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
