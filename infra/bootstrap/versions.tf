# The one root module that is applied by hand, once, before `infra/` can have remote state.
#
# State has to live somewhere, and the bucket that holds it cannot be created by the configuration
# that uses it: `terraform init` in `infra/` reads the bucket before anything is planned. So this is
# a root of its own, with its own state - the one place local state is the right answer, because
# this is what makes remote state possible. Keep that `terraform.tfstate` file somewhere it will not
# be lost: without it, the bucket still exists, but Terraform no longer knows it made it.
#
#   cd infra/bootstrap
#   terraform init
#   terraform apply -var state_bucket=<a globally unique name>
#   cd .. && cp backend.hcl.example backend.hcl   # fill in the bucket this prints
#   terraform init -backend-config=backend.hcl

terraform {
  # The same floor as `infra/`: one Terraform applies both, and `infra/`'s backend needs 1.11 for
  # S3 state locking (`use_lockfile`).
  required_version = ">= 1.11"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.60"
    }
  }
}

provider "aws" {
  region = var.region

  default_tags {
    tags = {
      Project     = var.project
      Environment = var.environment
      ManagedBy   = "terraform"
    }
  }
}
