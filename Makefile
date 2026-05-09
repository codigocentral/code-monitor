.PHONY: build build-release test lint fmt clean docker-build docker-up docker-down install-dev help

# Default target
.DEFAULT_GOAL := help

# Colors for output
BLUE := \033[36m
GREEN := \033[32m
YELLOW := \033[33m
RED := \033[31m
NC := \033[0m # No Color

help: ## Show this help message
	@echo "$(BLUE)Code Monitor - Available Commands$(NC)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(GREEN)%-20s$(NC) %s\n", $$1, $$2}'

# Build commands
build: ## Build the project in debug mode
	@echo "$(BLUE)Building project (debug)...$(NC)"
	cargo build --all

build-release: ## Build the project in release mode
	@echo "$(BLUE)Building project (release)...$(NC)"
	cargo build --all --release

# Testing commands
test: ## Run all tests
	@echo "$(BLUE)Running tests...$(NC)"
	cargo test --all

test-verbose: ## Run all tests with verbose output
	@echo "$(BLUE)Running tests (verbose)...$(NC)"
	cargo test --all -- --nocapture

# Linting and formatting
lint: ## Run clippy lints
	@echo "$(BLUE)Running clippy...$(NC)"
	cargo clippy --all-targets --all-features -- -D warnings

fmt: ## Format all code
	@echo "$(BLUE)Formatting code...$(NC)"
	cargo fmt --all

fmt-check: ## Check code formatting
	@echo "$(BLUE)Checking formatting...$(NC)"
	cargo fmt --all -- --check

# Cleaning
clean: ## Clean build artifacts
	@echo "$(BLUE)Cleaning build artifacts...$(NC)"
	cargo clean

# Development commands
install-dev: ## Install development dependencies (protoc, etc.)
	@echo "$(BLUE)Installing development dependencies...$(NC)"
	@echo "Please install protoc manually:"
	@echo "  - macOS: brew install protobuf"
	@echo "  - Ubuntu/Debian: sudo apt-get install protobuf-compiler"
	@echo "  - Windows: choco install protoc"

run-server: build ## Run the server in debug mode
	@echo "$(GREEN)Starting server...$(NC)"
	./target/debug/monitor-server

run-client: build ## Run the client in debug mode
	@echo "$(GREEN)Starting client...$(NC)"
	./target/debug/monitor-client

# Docker commands
docker-build: ## Build Docker images
	@echo "$(BLUE)Building Docker images...$(NC)"
	docker-compose build

docker-up: ## Start services with Docker Compose
	@echo "$(BLUE)Starting services...$(NC)"
	docker-compose up -d

docker-down: ## Stop services
	@echo "$(BLUE)Stopping services...$(NC)"
	docker-compose down

docker-logs: ## View Docker logs
	@echo "$(BLUE)Viewing logs...$(NC)"
	docker-compose logs -f

docker-clean: ## Remove Docker containers and volumes
	@echo "$(RED)Removing containers and volumes...$(NC)"
	docker-compose down -v

# Release commands
release-check: ## Check if ready for release
	@echo "$(BLUE)Checking release readiness...$(NC)"
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all
	@echo "$(GREEN)✓ Ready for release!$(NC)"

# TLS certificates (development)
certs: ## Generate self-signed certificates for development
	@echo "$(BLUE)Generating self-signed certificates...$(NC)"
	mkdir -p certs
	openssl req -x509 -newkey rsa:4096 \
		-keyout certs/server.key \
		-out certs/server.crt \
		-days 365 \
		-nodes \
		-subj "/CN=localhost" \
		-addext "subjectAltName=DNS:localhost,IP:127.0.0,1"
	@echo "$(GREEN)✓ Certificates generated in certs/$(NC)"

# Documentation
docs: ## Build and open documentation
	@echo "$(BLUE)Building documentation...$(NC)"
	cargo doc --all --no-deps --open
