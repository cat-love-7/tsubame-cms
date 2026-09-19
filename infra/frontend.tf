# The admin app, delivered: a private bucket behind CloudFront, and the API on the same origin.
#
# One origin is what makes the frontend simple: `apiUrl()` asks for `/api/...`, and this
# distribution answers `/api/*` from the function URL and everything else from the bucket - so the
# browser sees one origin, no CORS, and the paths the CMS serves are the paths it asks for (see
# `sl_cms_core::API_PREFIX`).

resource "aws_s3_bucket" "app" {
  bucket = var.frontend_bucket != "" ? var.frontend_bucket : "${local.name}-app"

  # The contents are a build artifact: `terraform destroy` may take them, and `aws s3 sync
  # --delete` is what keeps the bucket equal to the build.
  force_destroy = true
}

# The bucket is read through CloudFront and by nothing else.
resource "aws_s3_bucket_public_access_block" "app" {
  bucket = aws_s3_bucket.app.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_cloudfront_origin_access_control" "app" {
  name                              = "${local.name}-app"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

# Only this distribution may read, and only objects. No `s3:ListBucket`: nothing here reads a
# listing, and what a deployment puts in the bucket is its own build output.
resource "aws_s3_bucket_policy" "app" {
  bucket = aws_s3_bucket.app.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid       = "AllowCloudFront"
      Effect    = "Allow"
      Principal = { Service = "cloudfront.amazonaws.com" }
      Action    = "s3:GetObject"
      Resource  = "${aws_s3_bucket.app.arn}/*"
      Condition = {
        StringEquals = { "AWS:SourceArn" = aws_cloudfront_distribution.app.arn }
      }
    }]
  })

  depends_on = [aws_s3_bucket_public_access_block.app]
}

resource "aws_cloudfront_distribution" "app" {
  enabled             = true
  is_ipv6_enabled     = true
  default_root_object = "index.html"
  comment             = "${local.name} admin"
  aliases             = local.app_aliases

  origin {
    domain_name              = aws_s3_bucket.app.bucket_regional_domain_name
    origin_id                = "app"
    origin_access_control_id = aws_cloudfront_origin_access_control.app.id
  }

  origin {
    # The function URL, without its scheme or its trailing slash: CloudFront wants a domain.
    domain_name = trimsuffix(replace(aws_lambda_function_url.cms.function_url, "https://", ""), "/")
    origin_id   = "api"

    # A function URL is not an S3 origin, so it takes the generic configuration. HTTPS only, and
    # TLS 1.2 at least: the CMS is reached over the public internet.
    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }
  }

  # The app itself. Its cache policy lets each object say how long it may be kept
  # (`scripts/deploy-frontend.sh` gives the hashed bundles a year and `index.html` nothing), which
  # is what keeps a deployment from having to wait for a TTL.
  default_cache_behavior {
    target_origin_id       = "app"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    cache_policy_id        = aws_cloudfront_cache_policy.app.id
    compress               = true

    # A path with no file extension in it is a route, not a file: the app is one document.
    function_association {
      event_type   = "viewer-request"
      function_arn = aws_cloudfront_function.spa_routing.arn
    }
  }

  # The API. Nothing is cached (every answer is a token's answer), and the viewer's headers -
  # `Authorization` above all - are forwarded to the function.
  ordered_cache_behavior {
    path_pattern             = "/api/*"
    target_origin_id         = "api"
    viewer_protocol_policy   = "redirect-to-https"
    allowed_methods          = ["GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE"]
    cached_methods           = ["GET", "HEAD"]
    cache_policy_id          = data.aws_cloudfront_cache_policy.caching_disabled.id
    origin_request_policy_id = data.aws_cloudfront_origin_request_policy.all_viewer_except_host.id
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  # The operator's certificate, on the operator's name: the app is reached at `app_url`, and the
  # DNS record that points there is theirs to make (`terraform output frontend_url` says at what).
  viewer_certificate {
    acm_certificate_arn      = var.frontend_certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}

resource "aws_cloudfront_function" "spa_routing" {
  name    = "${local.name}-spa-routing"
  runtime = "cloudfront-js-2.0"
  comment = "Serve a path that names no file as index.html"
  publish = true
  code    = file("${path.module}/spa-routing.js")
}

# The app's objects are the ones that know their own age: the bundles are content-hashed and
# `index.html` is not, and the deployment says so with each upload. `MinTTL = 0` is what makes that
# possible - the managed caching policies floor a `no-cache` at an hour.
resource "aws_cloudfront_cache_policy" "app" {
  name        = "${local.name}-app"
  min_ttl     = 0
  default_ttl = 86400
  max_ttl     = 31536000

  parameters_in_cache_key_and_forwarded_to_origin {
    cookies_config {
      cookie_behavior = "none"
    }
    headers_config {
      header_behavior = "none"
    }
    query_strings_config {
      query_string_behavior = "none"
    }
    enable_accept_encoding_gzip   = true
    enable_accept_encoding_brotli = true
  }
}

data "aws_cloudfront_cache_policy" "caching_disabled" {
  name = "Managed-CachingDisabled"
}

data "aws_cloudfront_origin_request_policy" "all_viewer_except_host" {
  name = "Managed-AllViewerExceptHostHeader"
}
