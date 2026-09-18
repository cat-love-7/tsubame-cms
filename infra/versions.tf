terraform {
  # `use_lockfile` below arrived in 1.10, still experimental; 1.11 is the first version where the
  # S3 lock can be relied on. An older Terraform fails to read this block rather than applying
  # without a lock.
  required_version = ">= 1.11"

  # Remote state: two operators cannot apply two different plans, and the state is not a file on
  # one laptop. `use_lockfile` is S3's own locking - a `<key>.tflock` object beside the state - so
  # the DynamoDB table older configurations locked with is not needed (it is deprecated and will be
  # removed in a future Terraform).
  #
  # `bucket`, `key` and `region` are deliberately absent: the bucket has to exist before this block
  # is read, and one account's bucket does not belong in everyone's checkout. `infra/bootstrap/`
  # creates it, and `terraform init -backend-config=backend.hcl` fills the rest in (see
  # `backend.hcl.example`).
  backend "s3" {
    use_lockfile = true
    encrypt      = true
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.60"
    }
    archive = {
      source  = "hashicorp/archive"
      version = ">= 2.4"
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
