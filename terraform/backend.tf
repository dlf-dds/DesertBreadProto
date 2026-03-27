terraform {
  backend "s3" {
    bucket         = "cochlearis-infra-tf-state"
    key            = "desert-bread-proto/dev/terraform.tfstate"
    region         = "eu-central-1"
    dynamodb_table = "cochlearis-infra-tf-lock"
    encrypt        = true
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 4.0"
    }
  }

  required_version = ">= 1.5"
}
