locals {
  # One name for everything, so the table, the bucket, the pools and the function can be told
  # apart from another deployment's in the same account.
  name = "${var.project}-${var.environment}"

  # Cognito wants a unique prefix across the whole region, so the default can be overridden.
  cognito_domain_prefix = var.cognito_domain_prefix == "" ? local.name : var.cognito_domain_prefix

  # The artifact that matches the architecture the function is declared with: keeping the two in
  # one place is what stops a zip built for one architecture from being deployed as the other,
  # which Lambda only reports at invoke time.
  function_zip = (
    var.function_zip != ""
    ? var.function_zip
    : "${path.module}/build/tsubame-aws-${var.function_architecture}.zip"
  )

  # Where the app is served from, as a browser sees it. An input rather than something derived
  # from the distribution: the pool's client has to register the callback before the distribution
  # exists, and deriving it here would be a cycle (distribution → function URL → function →
  # client → callback → distribution).
  app_origin  = var.app_url
  app_aliases = [replace(var.app_url, "https://", "")]

  # Where the preview site is served from, and the name its distribution answers to. A name of its
  # own rather than a path under the app: a preview renders unpublished HTML and the app's origin
  # holds the editor's token in `localStorage`, so the two must not share an origin
  # (`docs/preview-site.md` §7). Empty `preview_url` means the conventional name under the app's.
  preview_origin = var.preview_url != "" ? var.preview_url : "https://preview.${replace(var.app_url, "https://", "")}"
  preview_host   = replace(local.preview_origin, "https://", "")

  # Where the browser signs in. `GET /auth/capabilities` reports it, so the client can send
  # someone there instead of describing where to go.
  # The hosted sign-in page. The screen does not link here as it stands: it builds the address from
  # this one, adding the PKCE challenge, a state and the redirect it registered - see
  # `frontend/src/app/core/auth/hosted-login.ts`.
  login_url = "https://${aws_cognito_user_pool_domain.cms.domain}.auth.${var.region}.amazoncognito.com/login?client_id=${aws_cognito_user_pool_client.browser.id}&response_type=code&scope=openid+email"

  # What the function needs to find its way around. Kept in one place because the code reads
  # exactly these names (crates/aws/src/settings.rs).
  # `AWS_REGION` is not set here: the Lambda runtime provides it, the key is reserved, and a
  # function configuration that names it is rejected.
  function_environment = merge(
    {
      RUST_LOG                  = "info"
      DYNAMODB_TABLE            = aws_dynamodb_table.cms.name
      S3_BUCKET                 = aws_s3_bucket.images.bucket
      COGNITO_USER_POOL_ID      = aws_cognito_user_pool.cms.id
      COGNITO_CLIENT_ID         = aws_cognito_user_pool_client.browser.id
      COGNITO_LOGIN_URL         = local.login_url
      BOOTSTRAP_ADMIN_USERNAMES = join(",", var.bootstrap_admin_usernames)
      # The app's own origin, plus the preview site: the preview is on a name of its own, so its
      # browser calls the API cross-origin and has to be allowed to. Added here rather than left to
      # the operator so the two cannot disagree.
      CORS_ALLOWED_ORIGINS = join(",", distinct(concat(var.cors_allowed_origins, [local.preview_origin])))
      # Where an admin screen sends a reviewer: `/auth/capabilities` reports it, and a deployment
      # that did not set it would have nothing readable to hand over (`docs/preview-site.md`).
      PREVIEW_SITE_URL = local.preview_origin
      # Only the ARN: the value is read from Secrets Manager at startup, so it is in neither the
      # function's configuration nor the state file.
      JWT_SECRET_ARN = var.jwt_secret_arn
      WEBHOOK_URLS   = join(",", var.webhook_urls)
      # Which of the two ways this deployment serves an image's bytes, and how long a signature
      # lasts when it does. `AWS_IMAGE_BASE_URL` (a CDN in front of the bucket) only means anything
      # for the address mode: a signature names the bucket itself.
      AWS_IMAGE_DELIVERY        = var.image_delivery
      AWS_IMAGE_URL_TTL_SECONDS = tostring(var.image_url_ttl_seconds)
    },
    var.webhook_secret == "" ? {} : { WEBHOOK_SECRET = var.webhook_secret },
    # Only when an operator has an opinion: the defaults belong to the application, and a
    # deployment that never mentions them should follow the application when they change.
    var.max_request_bytes == null ? {} : { MAX_REQUEST_BYTES = tostring(var.max_request_bytes) },
    var.max_image_bytes == null ? {} : { MAX_IMAGE_BYTES = tostring(var.max_image_bytes) },
    var.max_response_bytes == null
    ? {}
    : { MAX_RESPONSE_BYTES = tostring(var.max_response_bytes) },
    var.image_delivery == "public"
    ? {
      AWS_IMAGE_BASE_URL = "https://${aws_s3_bucket.images.bucket}.s3.${var.region}.amazonaws.com"
    }
    : {},
  )
}
