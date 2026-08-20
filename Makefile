.PHONY: help validate lint-markdown lint-yaml docs-serve docs-build clean

help:
	@echo "wetechi-netmon — Phase 1 targets (documentation/validation only)"
	@echo ""
	@echo "  make validate       Run all available validation (markdown + yaml lint)"
	@echo "  make lint-markdown  Lint all Markdown files with markdownlint-cli2"
	@echo "  make lint-yaml      Validate syntax of all YAML files"
	@echo "  make docs-serve     Serve the MkDocs Material site locally (requires Python)"
	@echo "  make docs-build     Build the MkDocs Material site strictly (requires Python)"
	@echo ""
	@echo "Rust (cargo) and frontend (npm build/test) targets are added"
	@echo "starting Phase 2 and Phase 6 respectively — see docs/roadmap.md."

validate: lint-markdown lint-yaml
	@echo "Phase 1 validation complete."

lint-markdown:
	npx -y markdownlint-cli2 "**/*.md" "#node_modules"

lint-yaml:
	npx -y js-yaml mkdocs.yml > /dev/null && echo "OK: mkdocs.yml"
	@for f in $$(find .github -type f \( -name '*.yml' -o -name '*.yaml' \)); do \
		npx -y js-yaml "$$f" > /dev/null && echo "OK: $$f" || exit 1; \
	done

docs-serve:
	mkdocs serve

docs-build:
	mkdocs build --strict

clean:
	rm -rf site/
