terraform {
  required_version = ">= 1.6"

  # State is kept *somewhere* before two people apply this at once. The block is commented out
  # because it needs a bucket that exists and a name that is unique: uncomment it, fill both in,
  # and run `terraform init -migrate-state`.
  #
  # backend "s3" {
  #   bucket         = "CHANGE-ME-terraform-state"
  #   key            = "sl-cms/terraform.tfstate"
  #   region         = "eu-west-1"
  #   dynamodb_table = "CHANGE-ME-terraform-locks"
  #   encrypt        = true
  # }

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
