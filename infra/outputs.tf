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
