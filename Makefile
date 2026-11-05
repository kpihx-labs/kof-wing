.PHONY: check doctor push run

check:
	@shellcheck scripts/*.sh
	@./scripts/doctor.sh

doctor:
	@./scripts/doctor.sh

run:
	@./scripts/run.sh

push:
	git push github main
