# Everything structured lives in one table (doc/aws-dynamodb-design.md): the keys are the
# layout, and there are deliberately no secondary indexes.
resource "aws_dynamodb_table" "cms" {
  name         = local.name
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "pk"
  range_key    = "sk"

  attribute {
    name = "pk"
    type = "S"
  }

  attribute {
    name = "sk"
    type = "S"
  }

  point_in_time_recovery {
    enabled = var.point_in_time_recovery
  }

  deletion_protection_enabled = var.deletion_protection
}

# Image bytes. The URL stored in content is the object's own address, so the bucket has to be
# readable — a presigned URL in a page would expire with the page
# (doc/aws-dynamodb-design.md, and `AwsSettings::image_url`).
resource "aws_s3_bucket" "images" {
  bucket = "${local.name}-images"
}

resource "aws_s3_bucket_public_access_block" "images" {
  bucket = aws_s3_bucket.images.id

  block_public_acls       = true
  block_public_policy     = false
  ignore_public_acls      = true
  restrict_public_buckets = false
}

resource "aws_s3_bucket_policy" "images_readable" {
  # Only in the mode that serves the object's own address. With `presigned`, the bucket stays
  # private and every URL the CMS hands out is a signature - the whole point of choosing it.
  count = var.image_delivery == "public" ? 1 : 0

  bucket = aws_s3_bucket.images.id

  # Reading is public; writing is not, which is what the presigned PUT depends on.
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = "*"
      Action    = ["s3:GetObject"]
      Resource  = "${aws_s3_bucket.images.arn}/*"
    }]
  })

  depends_on = [aws_s3_bucket_public_access_block.images]
}

resource "aws_s3_bucket_server_side_encryption_configuration" "images" {
  bucket = aws_s3_bucket.images.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

# Uploads expire halfway through if they are abandoned, and old versions have no purpose.
resource "aws_s3_bucket_lifecycle_configuration" "images" {
  bucket = aws_s3_bucket.images.id

  rule {
    id     = "abort-incomplete-uploads"
    status = "Enabled"

    filter {}

    abort_incomplete_multipart_upload {
      days_after_initiation = 7
    }
  }
}

resource "aws_s3_bucket_cors_configuration" "images" {
  bucket = aws_s3_bucket.images.id

  # The browser PUTs the bytes here directly, so this is where the upload's CORS is decided.
  cors_rule {
    allowed_methods = ["PUT"]
    # The app's own origin is always allowed: the browser PUTs to the presigned URL from there, and
    # that is cross-origin whatever the API's origin is.
    allowed_origins = distinct(concat(var.cors_allowed_origins, [local.app_origin]))
    allowed_headers = ["*"]
    max_age_seconds = 3000
  }
}
