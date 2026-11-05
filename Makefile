.PHONY: check doctor run

check:
	@shellcheck scripts/*.sh
	@./scripts/doctor.sh

doctor:
	@./scripts/doctor.sh

run:
	@./scripts/run.sh
