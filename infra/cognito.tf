# Cognito is the identity provider: who someone is. What they may do is the CMS's own records
# (doc/aws-plan.md, P4), so there are no groups to keep in step here.
resource "aws_cognito_user_pool" "cms" {
  name = local.name

  # Accounts are named by a username, and an email address is optional — the same rule the CMS
  # applies locally (crates/core/src/models/user.rs).
  username_attributes      = []
  auto_verified_attributes = []

  username_configuration {
    case_sensitive = false
  }

  # The same minimum the CMS enforces locally; the character classes are Cognito's default and
  # stricter, which is fine — a password that satisfies Cognito satisfies the CMS.
  password_policy {
    minimum_length = 8
  }

  # TOTP, optional: an account may turn it on, and nobody is locked out of a deployment because
  # they have not set it up yet. "Required" is a policy decision for whoever runs it; this is the
  # half that has to exist before that decision can be made at all.
  mfa_configuration = "OPTIONAL"

  software_token_mfa_configuration {
    enabled = true
  }

  admin_create_user_config {
    # An account an administrator creates has to change its password on first use.
    allow_admin_create_user_only = true
  }

  account_recovery_setting {
    recovery_mechanism {
      name     = "verified_email"
      priority = 1
    }
  }
}

resource "aws_cognito_user_pool_client" "browser" {
  name         = "${local.name}-browser"
  user_pool_id = aws_cognito_user_pool.cms.id

  # A browser cannot keep a secret, and this client is only ever used from one.
  generate_secret = false

  # SRP rather than sending the password to Cognito from JavaScript, plus refresh. The hosted
  # UI needs the authorization-code flow, which the pool supports by default.
  explicit_auth_flows = [
    "ALLOW_USER_SRP_AUTH",
    "ALLOW_REFRESH_TOKEN_AUTH",
  ]

  # The hosted sign-in page: **enabled here rather than assumed**. Without these the page answers
  # "redirect_uri is not registered" for every sign-in, which is what the CMS's sign-in link would
  # have led to.
  #
  # Authorization code with PKCE: this client is in a browser and cannot keep a secret, so the
  # proof that the code came back to the same caller is the verifier it generated before leaving.
  # The CMS exchanges the code for tokens (`POST /auth/cognito/exchange`) rather than the browser,
  # because the token endpoint answers without CORS headers.
  allowed_oauth_flows                  = ["code"]
  allowed_oauth_scopes                 = ["openid", "email"]
  allowed_oauth_flows_user_pool_client = true
  callback_urls                        = ["${local.app_origin}/auth/callback"]
  # Signing out of the CMS does not end the provider's session; this is where the provider sends
  # the browser when it is asked to end it.
  logout_urls = ["${local.app_origin}/login"]

  # Whether an account exists is none of a stranger's business: the same reason the CMS answers
  # "invalid username or password" for both cases.
  prevent_user_existence_errors = "ENABLED"

  supported_identity_providers = ["COGNITO"]

  token_validity_units {
    access_token  = "hours"
    id_token      = "hours"
    refresh_token = "days"
  }

  access_token_validity  = 1
  id_token_validity      = 1
  refresh_token_validity = 30
}

resource "aws_cognito_user_pool_domain" "cms" {
  domain       = local.cognito_domain_prefix
  user_pool_id = aws_cognito_user_pool.cms.id
}

# Brute force is handled in two places, and neither is a per-account counter of ours: Cognito
# locks an account after repeated failures, and this is the volume case that its documentation
# points at WAF for.
resource "aws_wafv2_web_acl" "sign_in" {
  name  = "${local.name}-sign-in"
  scope = "REGIONAL"

  default_action {
    allow {}
  }

  rule {
    name     = "rate-limit-sign-in"
    priority = 1

    action {
      block {}
    }

    statement {
      rate_based_statement {
        limit              = var.waf_rate_limit
        aggregate_key_type = "IP"
      }
    }

    visibility_config {
      cloudwatch_metrics_enabled = true
      metric_name                = "${local.name}-rate-limit"
      sampled_requests_enabled   = true
    }
  }

  visibility_config {
    cloudwatch_metrics_enabled = true
    metric_name                = "${local.name}-waf"
    sampled_requests_enabled   = true
  }
}

resource "aws_wafv2_web_acl_association" "sign_in" {
  resource_arn = aws_cognito_user_pool.cms.arn
  web_acl_arn  = aws_wafv2_web_acl.sign_in.arn
}
