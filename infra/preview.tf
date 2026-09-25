# The preview site, on a name of its own.
#
# A preview renders **unpublished** content: the working copy a signed link points at, plus the
# published things it references. That is a different thing to serve from the app, for one reason
# that decides the shape of this file: the app's origin keeps the editor's bearer token in
# `localStorage` (`frontend/src/app/core/auth/auth.service.ts`), and the app itself never
# renders CMS HTML. A preview does render it - the draft's Markdown - so a preview on the app's
# origin would turn any stored XSS in a draft into a stolen token. A separate distribution on a
# separate host name is what makes it a separate origin, and it is the only reason this exists
# rather than a `/preview` path (`docs/preview-site.md` §7).
#
# The bucket is the app's own; the preview site lives under the `preview/` prefix and this
# distribution's origin points at it, so nothing new is stored and `scripts/deploy-preview.sh` only
# ever touches that prefix.
#
# Sharing the bucket means the objects are *readable* through either distribution, so the app's
# distribution refuses `/preview/*` itself: `infra/app-routing.js` answers 404 for it, and the
# preview's own routing (`infra/spa-routing.js`) is a separate function because its routes begin with
# exactly that path. Without that guard the preview's script would be served from the app's origin,
# and the isolation above would be a convention rather than a boundary.

resource "aws_cloudfront_distribution" "preview" {
  enabled             = true
  is_ipv6_enabled     = true
  default_root_object = "index.html"
  comment             = "${local.name} preview"
  aliases             = [local.preview_host]

  origin {
    # The same bucket, read from `preview/`: a request for `/` reaches S3 as `preview/index.html`,
    # and a request for `/main-abc.js` as `preview/main-abc.js`.
    domain_name              = aws_s3_bucket.app.bucket_regional_domain_name
    origin_id                = "preview"
    origin_access_control_id = aws_cloudfront_origin_access_control.app.id
    origin_path              = "/preview"
  }

  # The preview site is one document with a router, exactly like the app: a path that names no file
  # is the shell, which is what lets `/preview/collections/blog/items/7` be a page. The same
  # `spa-routing.js` the app uses, because it rewrites to `/index.html` and the origin path is what
  # turns that into the preview site's own shell.
  default_cache_behavior {
    target_origin_id       = "preview"
    viewer_protocol_policy = "redirect-to-https"
    allowed_methods        = ["GET", "HEAD"]
    cached_methods         = ["GET", "HEAD"]
    # The app's policy, which lets each object say how long it may be kept: the deployment gives the
    # hashed bundles a year and `index.html` nothing. A preview is rebuilt rarely, so a TTL would
    # only delay the one deployment that matters.
    cache_policy_id = aws_cloudfront_cache_policy.app.id
    compress        = true

    function_association {
      event_type   = "viewer-request"
      function_arn = aws_cloudfront_function.spa_routing.arn
    }

    # A preview link is a credential in a query string, and an SPA answers 200 for every route it is
    # asked about: without this a crawler indexes the shell under whatever URL it guessed. The app's
    # function already says `noindex, nofollow` on every response, this distribution included.
    function_association {
      event_type   = "viewer-response"
      function_arn = aws_cloudfront_function.no_index.arn
    }
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  # `preview_certificate_arn` when the operator has one for this name, and the app's otherwise -
  # which works when it is a wildcard or carries both names in its SAN list. A name no certificate
  # covers is a distribution nobody can reach, which is why nothing here invents one.
  viewer_certificate {
    acm_certificate_arn      = var.preview_certificate_arn != "" ? var.preview_certificate_arn : var.frontend_certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}
