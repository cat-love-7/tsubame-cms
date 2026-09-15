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

  # Image bytes: the CMS signs uploads and deletes objects, and never reads them back (the
  # browser does that through the bucket's public read policy).
  statement {
    effect    = "Allow"
    actions   = ["s3:PutObject", "s3:DeleteObject"]
    resources = ["${aws_s3_bucket.images.arn}/*"]
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
