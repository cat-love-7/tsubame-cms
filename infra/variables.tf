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

variable "jwt_secret_arn" {
  description = <<-EOT
    The ARN of the Secrets Manager secret that signs preview links and password-reset links -
    the same secret the on-premises deployment uses. Cognito issues the sign-in tokens, so this
    one no longer signs those.

    The secret is created outside Terraform, on purpose: a value that passes through this
    configuration would sit in the state file, and in whatever carries it there. Create it once
    with

      aws secretsmanager create-secret --name sl-cms/jwt-secret \
        --secret-string "$(openssl rand -base64 48)"

    and put the ARN it prints here. The function reads it at startup; `JWT_SECRET` (used by a
    local run, and by the on-premises binary) is not set in this deployment.
  EOT
  type        = string
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
    Where the CMS screen is served from, as a browser sees it: the origin, without a trailing
    slash, for example `https://cms.example.com`.

    It cannot be derived from anything here. The identity provider sends the browser back to
    `<app_url>/auth/callback`, which has to be an address the pool's client was told about in
    advance - and the CloudFront distribution answers on that name, so the address has to be
    known before either exists. The DNS record pointing it here is the operator's, like the
    secret below: `terraform output frontend_url` is the distribution to point it at.
  EOT
  type        = string

  validation {
    condition     = can(regex("^https://[^/]+$", var.app_url))
    error_message = "app_url is an https origin without a trailing slash, for example https://cms.example.com."
  }
}

variable "frontend_certificate_arn" {
  description = <<-EOT
    An ACM certificate covering `app_url`. CloudFront certificates live in **us-east-1** whatever
    region the rest of the deployment is in, and there is no default: a distribution cannot be
    reached on a name no certificate covers.
  EOT
  type        = string

  validation {
    condition     = can(regex("^arn:aws:acm:us-east-1:", var.frontend_certificate_arn))
    error_message = "CloudFront reads certificates from us-east-1, so the ARN has to name that region."
  }
}

variable "frontend_bucket" {
  description = <<-EOT
    Name of the bucket the built app is synced to. Empty means `<project>-<environment>-app`, which
    is what the deployment creates; set it only when that name is taken in the account.
  EOT
  type        = string
  default     = ""
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
    # `floor` as well as the range: the application parses this variable as whole seconds
    # (`AWS_IMAGE_URL_TTL_SECONDS`), so a fraction would pass here and then refuse to start the
    # deployment.
    condition = (
      var.image_url_ttl_seconds == floor(var.image_url_ttl_seconds) &&
      var.image_url_ttl_seconds >= 60 &&
      var.image_url_ttl_seconds <= 604800
    )
    error_message = "A signature lasts a whole number of seconds, between a minute and seven days (604800), as SigV4 allows."
  }
}

variable "max_request_bytes" {
  description = <<-EOT
    The largest JSON request body the function accepts (`MAX_REQUEST_BYTES`). Left unset, the
    application's own default is used (1MB) - a large value on a platform that refuses an
    invocation over 6MB and base64-encodes what it sends would only turn one refusal into another.
  EOT
  type        = number
  default     = null

  validation {
    condition     = var.max_request_bytes == null || var.max_request_bytes >= 1
    error_message = "A request body limit is at least one byte."
  }
}

variable "max_image_bytes" {
  description = <<-EOT
    The largest image the CMS accepts, in bytes (`MAX_IMAGE_BYTES`). It is what the presigned
    upload is signed for, so S3 refuses anything else, and it is reported to clients through
    `GET /auth/capabilities` so a browser can refuse the file before sending it. Left unset, the
    application's own default is used (10MB).
  EOT
  type        = number
  default     = null

  validation {
    condition     = var.max_image_bytes == null || var.max_image_bytes >= 1
    error_message = "An image limit is at least one byte."
  }
}

variable "max_response_bytes" {
  description = <<-EOT
    The largest JSON response the function returns (`MAX_RESPONSE_BYTES`). The delivery API cuts
    a page of items to fit it and reports where to continue, so this is the graceful answer to
    Lambda's 6MB reply limit; a response that still does not fit (an admin list asked for whole,
    say) is refused with the CMS's own error rather than failing the invocation. Left unset, the
    application's own default is used (4MB).
  EOT
  type        = number
  default     = null

  validation {
    condition     = var.max_response_bytes == null || var.max_response_bytes >= 1
    error_message = "A response limit is at least one byte."
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
  description = <<-EOT
    Refuse to destroy the table. On by default: the table holds every account, collection, item
    and page, and `terraform destroy` is one word away from all of it. A scratch deployment that
    really does want it gone sets this to false (and most of them want a separate project name
    instead, so a mistake cannot reach production).
  EOT
  type        = bool
  default     = true
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
