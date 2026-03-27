terraform {
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
}

variable "relay_name" {
  description = "Short name for this relay (fra, isr, mum)"
  type        = string
}

variable "region" {
  description = "AWS region"
  type        = string
}

variable "zone_id" {
  description = "Cloudflare zone ID"
  type        = string
}

variable "dns_name" {
  description = "DNS record name (e.g., relay, relay-isr)"
  type        = string
}

variable "domain" {
  description = "Domain name"
  type        = string
}

variable "instance_type" {
  description = "EC2 instance type"
  type        = string
  default     = "t3.micro"
}

variable "tags" {
  description = "Resource tags"
  type        = map(string)
  default     = {}
}

# --- AMI ---

data "aws_ami" "ubuntu" {
  most_recent = true
  owners      = ["099720109477"] # Canonical

  filter {
    name   = "name"
    values = ["ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-amd64-server-*"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }
}

# --- Networking ---

# Create a standalone VPC for the relay (simple, isolated)
resource "aws_vpc" "relay" {
  cidr_block           = "10.${var.relay_name == "fra" ? 10 : var.relay_name == "isr" ? 11 : 12}.0.0/24"
  enable_dns_hostnames = true
  enable_dns_support   = true

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-vpc"
  })
}

resource "aws_internet_gateway" "relay" {
  vpc_id = aws_vpc.relay.id

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-igw"
  })
}

resource "aws_subnet" "relay" {
  vpc_id                  = aws_vpc.relay.id
  cidr_block              = aws_vpc.relay.cidr_block
  map_public_ip_on_launch = true

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-subnet"
  })
}

resource "aws_route_table" "relay" {
  vpc_id = aws_vpc.relay.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.relay.id
  }

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-rt"
  })
}

resource "aws_route_table_association" "relay" {
  subnet_id      = aws_subnet.relay.id
  route_table_id = aws_route_table.relay.id
}

# --- Security Group ---

resource "aws_security_group" "relay" {
  name_prefix = "relay-${var.relay_name}-"
  vpc_id      = aws_vpc.relay.id
  description = "iroh relay server - ${var.relay_name}"

  # SSH
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "SSH"
  }

  # HTTPS (iroh relay HTTP endpoint)
  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "HTTPS - iroh relay"
  }

  # QUIC (iroh relay QUIC transport)
  ingress {
    from_port   = 3478
    to_port     = 3478
    protocol    = "udp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "QUIC - iroh relay"
  }

  # All outbound
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
    description = "All outbound"
  }

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-sg"
  })
}

# --- SSH Key ---

resource "tls_private_key" "relay" {
  algorithm = "ED25519"
}

resource "aws_key_pair" "relay" {
  key_name_prefix = "relay-${var.relay_name}-"
  public_key      = tls_private_key.relay.public_key_openssh
}

resource "aws_secretsmanager_secret" "relay_ssh" {
  name_prefix = "relay-${var.relay_name}-ssh-"
  description = "SSH private key for relay ${var.relay_name}"
}

resource "aws_secretsmanager_secret_version" "relay_ssh" {
  secret_id     = aws_secretsmanager_secret.relay_ssh.id
  secret_string = tls_private_key.relay.private_key_openssh
}

# --- EC2 Instance ---

resource "aws_instance" "relay" {
  ami                    = data.aws_ami.ubuntu.id
  instance_type          = var.instance_type
  key_name               = aws_key_pair.relay.key_name
  subnet_id              = aws_subnet.relay.id
  vpc_security_group_ids = [aws_security_group.relay.id]

  root_block_device {
    volume_size = 20
    volume_type = "gp3"
    encrypted   = true
  }

  user_data = <<-EOF
    #!/bin/bash
    set -euo pipefail

    # System updates
    apt-get update && apt-get upgrade -y

    # Install iroh-relay
    # The iroh-relay binary is distributed via cargo install or GitHub releases
    curl -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    cargo install iroh-relay

    # Configure UFW
    ufw default deny incoming
    ufw default allow outgoing
    ufw allow 22/tcp    # SSH
    ufw allow 443/tcp   # HTTPS
    ufw allow 3478/udp  # QUIC
    ufw --force enable

    # Create systemd service for iroh-relay
    cat > /etc/systemd/system/iroh-relay.service <<'SERVICE'
    [Unit]
    Description=iroh relay server
    After=network-online.target
    Wants=network-online.target

    [Service]
    Type=simple
    ExecStart=/root/.cargo/bin/iroh-relay --hostname ${var.dns_name}.${var.domain}
    Restart=always
    RestartSec=5

    [Install]
    WantedBy=multi-user.target
    SERVICE

    systemctl daemon-reload
    systemctl enable --now iroh-relay
  EOF

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}"
    Role = "iroh-relay"
  })
}

# --- Elastic IP ---

resource "aws_eip" "relay" {
  instance = aws_instance.relay.id
  domain   = "vpc"

  tags = merge(var.tags, {
    Name = "relay-${var.relay_name}-eip"
  })
}

# --- DNS ---

resource "cloudflare_record" "relay" {
  zone_id = var.zone_id
  name    = var.dns_name
  content = aws_eip.relay.public_ip
  type    = "A"
  ttl     = 300
  proxied = false # MUST be false for QUIC/gRPC
}

# --- Outputs ---

output "public_ip" {
  value = aws_eip.relay.public_ip
}

output "relay_url" {
  value = "https://${var.dns_name}.${var.domain}"
}

output "instance_id" {
  value = aws_instance.relay.id
}

output "ssh_key_secret" {
  value = aws_secretsmanager_secret.relay_ssh.name
}
