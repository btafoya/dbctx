# dbctx Database Lab Makefile
# Provides convenient targets for managing the Docker-based compatibility lab.

.PHONY: up down restart reset clean logs status verify benchmark ui admin certs help

# Default profile brings up all database services.
up: certs
	docker compose --profile all up -d
	docker compose --profile all ps

# Stop all services and remove containers.
down:
	docker compose --profile all down --remove-orphans

# Restart all services.
restart: down up

# Destroy all persistent data (volumes) and recreate fresh containers.
reset: down
	docker compose --profile all down -v --remove-orphans
	$(MAKE) up

# Remove all containers, volumes, images, and orphaned resources built by this lab.
clean:
	docker compose --profile all down -v --remove-orphans --rmi local

# Tail logs from all running services.
logs:
	docker compose --profile all logs -f

# Show the status of every service.
status:
	docker compose --profile all ps
	docker compose --profile all ps --format json 2>/dev/null || true

# Wait for all enabled databases to be healthy, then run the verification suite.
verify: up wait
	bash scripts/verify.sh

# Wait for all enabled databases to become healthy.
wait:
	bash scripts/wait.sh

# Run the benchmark suite and produce benchmark.md.
benchmark: up wait
	bash scripts/benchmark.sh

# Start the Adminer UI profile.
ui:
	docker compose --profile ui up -d
	docker compose --profile ui ps

# Start the full administration UI profile (Adminer + pgAdmin + phpMyAdmin).
admin:
	docker compose --profile admin up -d
	docker compose --profile admin ps

# Generate development TLS certificates if they do not already exist.
certs:
	bash scripts/init-certs.sh

# Print a short usage summary.
help:
	@echo "dbctx Database Lab"
	@echo ""
	@echo "Targets:"
	@echo "  up         Start all database services"
	@echo "  down       Stop all services"
	@echo "  restart    Restart all services"
	@echo "  reset      Destroy and recreate all persistent data"
	@echo "  clean      Remove containers, volumes, and images"
	@echo "  logs       Tail logs"
	@echo "  status     Show service status"
	@echo "  wait       Wait for all databases to be healthy"
	@echo "  verify     Run the verification suite"
	@echo "  benchmark  Run benchmarks and write benchmark.md"
	@echo "  ui         Start Adminer"
	@echo "  admin      Start Adminer + pgAdmin + phpMyAdmin"
	@echo "  certs      Generate development TLS certificates"
