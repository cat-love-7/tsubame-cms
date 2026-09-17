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
    : "${path.module}/build/sl-cms-aws-${var.function_architecture}.zip"
  )

  # Where the browser signs in. `GET /auth/capabilities` reports it, so the client can send
  # someone there instead of describing where to go.
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
      CORS_ALLOWED_ORIGINS      = join(",", var.cors_allowed_origins)
      JWT_SECRET                = var.jwt_secret
      WEBHOOK_URLS              = join(",", var.webhook_urls)
      AWS_IMAGE_BASE_URL        = "https://${aws_s3_bucket.images.bucket}.s3.${var.region}.amazonaws.com"
    },
    var.webhook_secret == "" ? {} : { WEBHOOK_SECRET = var.webhook_secret },
  )
}
