# The bucket behind `backend "s3"` in `infra/versions.tf`.
#
# Nothing here is about the CMS. What it holds is the state of the deployment - and since
# `WEBHOOK_SECRET` reaches the function through its environment, a deployment that sets one has a
# secret in that state, which is why the bucket is private, encrypted and hard to delete by hand.

resource "aws_s3_bucket" "state" {
  bucket = var.state_bucket

  # Removing four lines should be a decision rather than a keystroke: this bucket holds the only
  # record of what the deployment is.
  lifecycle {
    prevent_destroy = true
  }
}

# Versioning is what makes a bad apply recoverable - the state from before it is the object's
# previous version - and Terraform's own documentation asks for it. The lockfile an apply holds
# (`<key>.tflock`, see `use_lockfile`) lives in the same bucket.
resource "aws_s3_bucket_versioning" "state" {
  bucket = aws_s3_bucket.state.id

  versioning_configuration {
    status = "Enabled"
  }
}

# Old versions are the recovery path, not an archive: a state file is small, so a quarter's worth
# costs nothing and answers "what did this look like before that apply?".
resource "aws_s3_bucket_lifecycle_configuration" "state" {
  bucket = aws_s3_bucket.state.id

  rule {
    id     = "expire-old-state-versions"
    status = "Enabled"

    filter {}

    noncurrent_version_expiration {
      noncurrent_days = 90
    }
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "state" {
  bucket = aws_s3_bucket.state.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "state" {
  bucket = aws_s3_bucket.state.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# The public access block is the mechanism; this is the sentence a reviewer reads. Who may read the
# state is decided by the identity policy on the operator's credentials, which is where a
# per-deployment restriction belongs.
resource "aws_s3_bucket_policy" "state" {
  bucket = aws_s3_bucket.state.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid       = "DenyInsecureTransport"
      Effect    = "Deny"
      Principal = "*"
      Action    = "s3:*"
      Resource = [
        aws_s3_bucket.state.arn,
        "${aws_s3_bucket.state.arn}/*",
      ]
      Condition = {
        Bool = { "aws:SecureTransport" = "false" }
      }
    }]
  })

  depends_on = [aws_s3_bucket_public_access_block.state]
}
