# The function's logs, kept for a while and then forgotten: a deployment that logs forever is a
# deployment whose bill nobody can explain.
resource "aws_cloudwatch_log_group" "function" {
  name              = "/aws/lambda/${local.name}"
  retention_in_days = var.log_retention_days
}

data "aws_iam_policy_document" "assume_lambda" {
  statement {
    effect  = "Allow"
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "function" {
  name               = "${local.name}-function"
  assume_role_policy = data.aws_iam_policy_document.assume_lambda.json
}

data "aws_iam_policy_document" "function" {
  # Reading and writing the CMS's own table: the adapter only ever uses GetItem, PutItem,
  # DeleteItem, Query, UpdateItem and TransactWriteItems (doc/aws-dynamodb-design.md).
  statement {
    effect = "Allow"
    actions = [
      "dynamodb:GetItem",
      "dynamodb:PutItem",
      "dynamodb:UpdateItem",
      "dynamodb:DeleteItem",
      "dynamodb:Query",
      "dynamodb:TransactWriteItems",
    ]
    resources = [aws_dynamodb_table.cms.arn]
  }

  # Image bytes: the CMS signs uploads, deletes objects, and *looks one up* before pointing a
  # record at it (`ImageService::replace_image`).
  #
  # `s3:GetObject` is what HeadObject needs. It is also granted to everyone by the bucket's read
  # policy, which is how the CMS's own calls have been succeeding - but a bucket that stops being
  # public (behind CloudFront, say) would take the role's permission with it, so it is stated here
  # as well.
  statement {
    effect    = "Allow"
    actions   = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
    resources = ["${aws_s3_bucket.images.arn}/*"]
  }

  # ...and `s3:ListBucket` is what makes a *missing* key answer 404 rather than 403: S3 refuses to
  # tell a caller whether an object exists if it cannot list the bucket, and the code reads 404 as
  # "the upload is not there yet" while anything else is a failure. Without this, a replacement
  # whose upload never arrived is reported as an internal error instead of being refused.
  statement {
    effect    = "Allow"
    actions   = ["s3:ListBucket"]
    resources = [aws_s3_bucket.images.arn]
  }

  # The signing secret, read once at startup (`resolve_jwt_secret`). Scoped to the one secret:
  # the function has no business reading any other.
  statement {
    effect    = "Allow"
    actions   = ["secretsmanager:GetSecretValue"]
    resources = [var.jwt_secret_arn]
  }

  statement {
    effect    = "Allow"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.function.arn}:*"]
  }
}

resource "aws_iam_role_policy" "function" {
  name   = "${local.name}-function"
  role   = aws_iam_role.function.id
  policy = data.aws_iam_policy_document.function.json
}

resource "aws_lambda_function" "cms" {
  function_name = local.name
  role          = aws_iam_role.function.arn
  handler       = "bootstrap"
  runtime       = "provided.al2023"
  architectures = [var.function_architecture]

  filename         = local.function_zip
  source_code_hash = filebase64sha256(local.function_zip)

  memory_size = var.function_memory_mb
  timeout     = var.function_timeout_seconds

  environment {
    variables = local.function_environment
  }

  # The log group is declared above, so the function must not create its own first.
  depends_on = [aws_cloudwatch_log_group.function]
}

# A function URL rather than API Gateway: no stage, no route table, and its 6MB payload limit is
# the one the code already enforces a body size against (crates/aws/src/lambda.rs).
resource "aws_lambda_function_url" "cms" {
  function_name      = aws_lambda_function.cms.function_name
  authorization_type = "NONE" # the CMS checks the token itself, per route

  cors {
    allow_credentials = false
    allow_origins     = var.cors_allowed_origins
    allow_methods     = ["GET", "POST", "PATCH", "PUT", "DELETE", "OPTIONS"]
    allow_headers     = ["authorization", "content-type"]
    max_age           = 3000
  }
}

resource "aws_lambda_permission" "public_url" {
  statement_id           = "AllowPublicFunctionUrl"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.cms.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}

# A public function URL needs *both* actions: without this one, every request is answered with
# 403 before the function runs, even though the URL itself is public. `invoked_via_function_url`
# keeps the grant to the URL, so it is not also a permission to call the function directly.
resource "aws_lambda_permission" "public_url_invoke" {
  statement_id             = "AllowPublicFunctionUrlInvoke"
  action                   = "lambda:InvokeFunction"
  function_name            = aws_lambda_function.cms.function_name
  principal                = "*"
  invoked_via_function_url = true
}
