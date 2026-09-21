output "api_url" {
  description = "The API the browser talks to (the function URL)."
  value       = aws_lambda_function_url.cms.function_url
}

output "capabilities_url" {
  description = "What this deployment can do, which the UI asks before it draws anything."
  value       = "${aws_lambda_function_url.cms.function_url}auth/capabilities"
}

output "login_url" {
  description = "The hosted sign-in page; the same URL the function reports as `login_url`."
  value       = local.login_url
}

output "images_base_url" {
  description = "Where an uploaded image is readable from; what content stores."
  value       = "https://${aws_s3_bucket.images.bucket}.s3.${var.region}.amazonaws.com"
}

output "table_name" {
  description = "The DynamoDB table holding everything structured."
  value       = aws_dynamodb_table.cms.name
}

output "function_environment" {
  description = <<-EOT
    The environment the function runs with. Printed so an operator can see what the deployment
    was told, without opening the console — `crates/aws/src/settings.rs` reads exactly these.
  EOT
  value       = local.function_environment
  sensitive   = true
}

output "frontend_url" {
  description = "The distribution the app's DNS record should point at (a CNAME/alias target)."
  value       = "https://${aws_cloudfront_distribution.app.domain_name}"
}

output "frontend_bucket" {
  description = "The bucket `scripts/deploy-frontend.sh` syncs the build into."
  value       = aws_s3_bucket.app.id
}

output "frontend_distribution_id" {
  description = "The distribution to invalidate after a deployment."
  value       = aws_cloudfront_distribution.app.id
}

output "preview_site_url" {
  description = <<-EOT
    Where a shared preview link should be opened: `preview_url` when the operator named a site, or
    the conventional `preview.<app_url's host>` otherwise. The same value the function reports as
    `preview_site_url` in `/auth/capabilities`.
  EOT
  value       = local.preview_origin
}

output "preview_url" {
  description = "The distribution the preview site's DNS record should point at (a CNAME/alias target)."
  value       = "https://${aws_cloudfront_distribution.preview.domain_name}"
}

output "preview_distribution_id" {
  description = "The preview distribution to invalidate after a deployment."
  value       = aws_cloudfront_distribution.preview.id
}
